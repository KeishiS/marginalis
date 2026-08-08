use marginalis_application::{NoteLinks, NoteSyncEntry, NoteSyncPhase, NoteSyncRemovalReason};

use super::*;

#[tokio::test]
async fn snapshot_transitions_to_changes_without_missing_a_concurrent_update() {
    let database = database().await;
    let alice = user("alice");
    let id = note_id("0197c9bc-0000-7000-8000-000000000081");
    database
        .create_note(
            &note_seed("0197c9bc-0000-7000-8000-000000000081", "alice", "First").build(),
            NoteLinks::default(),
        )
        .await
        .expect("create");

    let snapshot = database
        .sync_notes_page(
            &alice,
            None,
            50,
            "snapshot-next-cursor-token-0000000000000001",
            UnixMillis::new(1_000),
        )
        .await
        .expect("snapshot");
    assert_eq!(snapshot.phase, NoteSyncPhase::Snapshot);
    assert!(!snapshot.has_more);
    assert!(
        matches!(&snapshot.entries[..], [NoteSyncEntry::Upsert(note)] if note.title() == "First")
    );

    database
        .update_visible_note(
            &alice,
            id,
            Revision::INITIAL,
            &draft("Second", "= Second", &[]),
            NoteLinks::default(),
            UnixMillis::new(1_100),
        )
        .await
        .expect("update during synchronization");
    let changes = database
        .sync_notes_page(
            &alice,
            Some(&snapshot.next_cursor),
            50,
            "changes-next-cursor-token-00000000000000001",
            UnixMillis::new(1_101),
        )
        .await
        .expect("changes");
    assert_eq!(changes.phase, NoteSyncPhase::Changes);
    assert!(
        matches!(&changes.entries[..], [NoteSyncEntry::Upsert(note)] if note.title() == "Second")
    );
}

#[tokio::test]
async fn access_revocation_never_returns_source_and_tombstone_survives_note_purge() {
    let database = database().await;
    let alice = user("alice");
    let bob = user("bob");
    let id = note_id("0197c9bc-0000-7000-8000-000000000082");
    database
        .create_note(
            &note_seed("0197c9bc-0000-7000-8000-000000000082", "alice", "Shared")
                .source("secret")
                .build(),
            NoteLinks::default(),
        )
        .await
        .expect("create");
    database
        .replace_note_acl(
            &alice,
            id,
            &[acl_entry("bob", NotePermission::Read)],
            Revision::INITIAL,
            UnixMillis::new(200),
        )
        .await
        .expect("share");
    let snapshot = database
        .sync_notes_page(
            &bob,
            None,
            50,
            "bob-snapshot-cursor-token-000000000000000001",
            UnixMillis::new(300),
        )
        .await
        .expect("bob snapshot");
    database
        .replace_note_acl(&alice, id, &[], revision(2), UnixMillis::new(400))
        .await
        .expect("revoke");
    let removed = database
        .sync_notes_page(
            &bob,
            Some(&snapshot.next_cursor),
            50,
            "bob-remove-cursor-token-0000000000000000001",
            UnixMillis::new(401),
        )
        .await
        .expect("remove");
    assert_eq!(
        removed.entries,
        vec![NoteSyncEntry::Remove {
            note_id: id,
            reason: NoteSyncRemovalReason::AccessRevoked
        }]
    );

    database
        .soft_delete_visible_note(&alice, id, revision(3), UnixMillis::new(500))
        .await
        .expect("delete");
    database
        .purge_deleted_before(UnixMillis::new(501))
        .await
        .expect("purge note");
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT reason FROM note_sync_changes WHERE issuer = ? AND subject = ? AND note_id = ?"
        )
        .bind(alice.issuer())
        .bind(alice.subject())
        .bind(id.to_string())
        .fetch_one(&database.pool)
        .await
        .expect("tombstone"),
        "deleted"
    );
}

#[tokio::test]
async fn cursor_is_bound_to_the_actor_and_expiry_is_typed() {
    let database = database().await;
    let alice = user("alice");
    let bob = user("bob");
    let page = database
        .sync_notes_page(
            &alice,
            None,
            50,
            "actor-bound-cursor-token-000000000000000001",
            UnixMillis::new(10),
        )
        .await
        .expect("cursor");
    assert_eq!(
        database
            .sync_notes_page(
                &bob,
                Some(&page.next_cursor),
                50,
                "unused-next-cursor-token-000000000000000001",
                UnixMillis::new(11)
            )
            .await,
        Err(marginalis_application::NoteSyncRepositoryError::InvalidCursor)
    );
    assert_eq!(
        database
            .sync_notes_page(
                &alice,
                Some(&page.next_cursor),
                50,
                "unused-next-cursor-token-000000000000000002",
                page.cursor_expires_at
            )
            .await,
        Err(marginalis_application::NoteSyncRepositoryError::CursorExpired)
    );
}

#[tokio::test]
async fn one_thousand_notes_are_paginated_without_duplicates() {
    let database = database().await;
    let alice = user("alice");
    for index in 0..1_000_u32 {
        let id = format!("0197c9bc-0000-7000-8000-{index:012x}");
        database
            .create_note(
                &note_seed(&id, "alice", &format!("Note {index}")).build(),
                NoteLinks::default(),
            )
            .await
            .expect("create note");
    }
    let mut cursor = None;
    let mut ids = std::collections::HashSet::new();
    loop {
        let next = format!("page-cursor-token-{number:040}", number = ids.len());
        let page = database
            .sync_notes_page(
                &alice,
                cursor.as_deref(),
                100,
                &next,
                UnixMillis::new(2_000),
            )
            .await
            .expect("page");
        assert_eq!(page.phase, NoteSyncPhase::Snapshot);
        for entry in page.entries {
            let NoteSyncEntry::Upsert(note) = entry else {
                panic!("snapshot entry")
            };
            assert!(ids.insert(note.note_id()));
        }
        cursor = Some(page.next_cursor);
        if !page.has_more {
            break;
        }
    }
    assert_eq!(ids.len(), 1_000);
    let changes = database
        .sync_notes_page(
            &alice,
            cursor.as_deref(),
            100,
            "after-snapshot-cursor-token-00000000000000001",
            UnixMillis::new(2_001),
        )
        .await
        .expect("changes");
    assert_eq!(changes.phase, NoteSyncPhase::Changes);
    assert!(changes.entries.is_empty());
}
