use marginalis_application::{BibliographyRepository, NoteGraphQuery, NoteLinks};
use marginalis_domain::{BibliographyItem, BibliographyItemId};

use super::*;

async fn snapshot_access(
    database: &SqliteDatabase,
    actor: &Actor,
    note_id: NoteId,
) -> Result<Option<NoteAccess>, SqliteStoreError> {
    Ok(database
        .note_view_snapshot(actor, note_id)
        .await?
        .map(|snapshot| snapshot.access))
}

#[tokio::test]
async fn single_source_updates_and_purges_notes_transactionally() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("schema initialization succeeds");
    let note_id = NoteId::new(
        EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("v7 note ID"),
    );
    let note = Note::restore(
        note_id,
        Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner"),
        "First title".into(),
        "first body".into(),
        vec!["research".into()],
        UnixMillis::new(100),
        UnixMillis::new(100),
        Revision::INITIAL,
        None,
    )
    .expect("consistent note");
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create note");
    assert_eq!(database.note(note_id, false).await, Ok(Some(note.clone())));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'note_acl'"
        )
        .fetch_one(&database.pool)
        .await
        .expect("schema query"),
        1
    );
    let alice = actor("https://id.example.test", "alice");
    let charlie = actor("https://id.example.test", "charlie");
    let same_subject_different_issuer = actor("https://other-id.example.test", "alice");
    let former_administrator = actor("https://id.example.test", "administrator");
    assert!(
        database
            .visible_note(&alice, note_id)
            .await
            .expect("owner is visible")
            .is_some()
    );
    assert_eq!(database.visible_note(&charlie, note_id).await, Ok(None));
    assert_eq!(
        database
            .visible_note(&same_subject_different_issuer, note_id)
            .await,
        Ok(None)
    );
    let owner_list = database
        .list_visible_notes(&alice)
        .await
        .expect("owner list");
    assert_eq!(owner_list.len(), 1);
    assert_eq!(owner_list[0].access, NoteAccess::Manage);
    assert!(
        database
            .list_visible_notes(&charlie)
            .await
            .expect("non-owner list")
            .is_empty()
    );
    assert!(
        database
            .list_visible_notes(&same_subject_different_issuer)
            .await
            .expect("different issuer list")
            .is_empty()
    );
    assert!(
        database
            .list_visible_notes(&former_administrator)
            .await
            .expect("unshared former administrator list")
            .is_empty()
    );
    assert_eq!(
        database
            .update_visible_note(
                &charlie,
                note_id,
                revision(1),
                &NoteDraft {
                    title: "Unauthorized title".into(),
                    source: "= Denied\n\nmust not persist".into(),
                    tags: vec![],
                },
                NoteLinks::default(),
                UnixMillis::new(150),
            )
            .await,
        Err(SqliteStoreError::NotFound)
    );
    assert_eq!(
        database
            .note(note_id, false)
            .await
            .expect("note query")
            .expect("note remains")
            .title(),
        "First title"
    );

    let updated = database
        .update_visible_note(
            &alice,
            note_id,
            revision(1),
            &NoteDraft {
                title: "Updated title".into(),
                source: "= Updated\n\nupdated body".into(),
                tags: vec!["research".into(), "v3".into()],
            },
            NoteLinks::default(),
            UnixMillis::new(200),
        )
        .await
        .expect("update note");
    assert_eq!(updated.revision().get(), 2);
    assert_eq!(updated.title(), "Updated title");
    assert_eq!(
        database
            .soft_delete_visible_note(&alice, note_id, revision(1), UnixMillis::new(300))
            .await,
        Err(SqliteStoreError::Conflict)
    );
    let deleted = database
        .soft_delete_visible_note(&alice, note_id, revision(2), UnixMillis::new(300))
        .await
        .expect("soft delete");
    assert_eq!(deleted.deleted_at(), Some(UnixMillis::new(300)));
    assert_eq!(database.note(note_id, false).await, Ok(None));
    let deleted = database
        .note(note_id, true)
        .await
        .expect("read deleted")
        .expect("deleted note remains");
    assert_eq!(deleted.deleted_at(), Some(UnixMillis::new(300)));
    assert_eq!(deleted.revision().get(), 3);

    let restored = database
        .restore_visible_note(&alice, note_id, revision(3), UnixMillis::new(350))
        .await
        .expect("restore note");
    assert_eq!(restored.deleted_at(), None);
    assert_eq!(restored.revision().get(), 4);
    database
        .replace_note_acl(
            &alice,
            note_id,
            &[NoteAclEntry::new(
                Identity::new("https://id.example.test".into(), "bob".into())
                    .expect("ACL identity"),
                NotePermission::Read,
            )],
            revision(4),
            UnixMillis::new(360),
        )
        .await
        .expect("store ACL");
    let snapshot = database
        .export_archive_snapshot()
        .await
        .expect("export snapshot");
    let plan = RestorePlan::new(snapshot.clone(), vec![(note_id, note_id)], Vec::new())
        .expect("valid restore plan");
    let imported_database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("empty import target");
    imported_database
        .restore(&plan)
        .await
        .expect("import snapshot");
    let restored_snapshot = imported_database
        .export_archive_snapshot()
        .await
        .expect("re-export snapshot");
    assert_eq!(restored_snapshot, snapshot);
    let view = imported_database
        .note_view_snapshot(&alice, note_id)
        .await
        .expect("imported view")
        .expect("imported note is visible");
    assert_eq!(view.related.outgoing.len(), 1);
    assert_eq!(view.related.incoming.len(), 1);
    assert_eq!(
        imported_database.restore(&plan).await,
        Err(SqliteStoreError::ArchiveTargetNotEmpty)
    );
    let nonempty_auth_database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("auth-state import target");
    nonempty_auth_database
        .oidc_login_attempt_store()
        .issue(
            OidcLoginAttempt {
                state: "pending-state".into(),
                nonce: "nonce".into(),
                pkce_verifier: "verifier".into(),
                expires_at: UnixMillis::new(1_000),
            },
            UnixMillis::new(0),
        )
        .await
        .expect("pending login attempt");
    assert_eq!(
        nonempty_auth_database.restore(&plan).await,
        Err(SqliteStoreError::ArchiveTargetNotEmpty)
    );
    let rejected_database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("empty rejected target");
    let empty_snapshot = rejected_database
        .export_archive_snapshot()
        .await
        .expect("empty snapshot");
    assert!(empty_snapshot.notes().is_empty());
    assert!(empty_snapshot.note_acl().is_empty());
    database
        .soft_delete_visible_note(&alice, note_id, revision(5), UnixMillis::new(400))
        .await
        .expect("delete before purge");
    assert_eq!(
        database
            .restore_visible_note(
                &alice,
                note_id,
                revision(6),
                UnixMillis::new(400 + SOFT_DELETE_RETENTION_MS + 1)
            )
            .await,
        Err(SqliteStoreError::Conflict)
    );
    assert_eq!(
        database
            .purge_deleted_before(UnixMillis::new(401))
            .await
            .expect("purge"),
        1
    );
    assert_eq!(database.note(note_id, true).await, Ok(None));
}

#[tokio::test]
async fn note_access_levels_follow_one_decision_table_and_acl_failures_roll_back() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("database");
    let note_id = NoteId::new(
        EntityId::from_str("0197c9bc-0000-7000-8000-000000000011").expect("v7 note ID"),
    );
    let owner_identity =
        Identity::new("https://id.example.test".into(), "owner".into()).expect("owner");
    let note = Note::restore(
        note_id,
        owner_identity.clone(),
        "Title".into(),
        "Body".into(),
        Vec::new(),
        UnixMillis::new(100),
        UnixMillis::new(100),
        Revision::INITIAL,
        None,
    )
    .expect("note");
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create");

    let owner = Actor::new(owner_identity);
    let reader = actor("https://id.example.test", "reader");
    let same_subject_other_issuer = actor("https://other-id.example.test", "reader");
    assert_eq!(
        snapshot_access(&database, &owner, note_id).await,
        Ok(Some(NoteAccess::Manage))
    );
    assert_eq!(snapshot_access(&database, &reader, note_id).await, Ok(None));

    let read_grant = NoteAclEntry::new(
        Identity::new("https://id.example.test".into(), "reader".into()).expect("reader"),
        NotePermission::Read,
    );
    let changed = database
        .replace_note_acl(
            &owner,
            note_id,
            &[read_grant],
            Revision::INITIAL,
            UnixMillis::new(110),
        )
        .await
        .expect("read ACL");
    assert_eq!(
        snapshot_access(&database, &reader, note_id).await,
        Ok(Some(NoteAccess::Read))
    );
    assert_eq!(
        snapshot_access(&database, &same_subject_other_issuer, note_id).await,
        Ok(None)
    );
    assert_eq!(
        database
            .update_visible_note(
                &reader,
                note_id,
                changed.revision(),
                &NoteDraft {
                    title: "Denied".into(),
                    source: "= Denied\n".into(),
                    tags: Vec::new(),
                },
                NoteLinks::default(),
                UnixMillis::new(120),
            )
            .await,
        Err(SqliteStoreError::NotFound)
    );

    let invalid_cross_issuer = NoteAclEntry::new(
        Identity::new("https://other-id.example.test".into(), "reader".into())
            .expect("other issuer"),
        NotePermission::Edit,
    );
    assert!(
        database
            .replace_note_acl(
                &owner,
                note_id,
                &[invalid_cross_issuer],
                changed.revision(),
                UnixMillis::new(130),
            )
            .await
            .is_err()
    );
    let unchanged = database
        .visible_note(&owner, note_id)
        .await
        .expect("read after rollback")
        .expect("note");
    assert_eq!(unchanged.revision(), changed.revision());
    assert_eq!(
        snapshot_access(&database, &reader, note_id).await,
        Ok(Some(NoteAccess::Read))
    );

    let edit_grant = NoteAclEntry::new(
        Identity::new("https://id.example.test".into(), "reader".into()).expect("reader"),
        NotePermission::Edit,
    );
    let changed = database
        .replace_note_acl(
            &owner,
            note_id,
            &[edit_grant],
            unchanged.revision(),
            UnixMillis::new(140),
        )
        .await
        .expect("edit ACL");
    assert_eq!(
        snapshot_access(&database, &reader, note_id).await,
        Ok(Some(NoteAccess::Edit))
    );
    assert!(
        database
            .update_visible_note(
                &reader,
                note_id,
                changed.revision(),
                &NoteDraft {
                    title: "Edited".into(),
                    source: "= Edited\n".into(),
                    tags: Vec::new(),
                },
                NoteLinks::default(),
                UnixMillis::new(150),
            )
            .await
            .is_ok()
    );
    assert_eq!(
        database
            .replace_note_acl(
                &reader,
                note_id,
                &[],
                Revision::new(changed.revision().get() + 1).expect("revision"),
                UnixMillis::new(160),
            )
            .await,
        Err(SqliteStoreError::NotFound)
    );
}

#[tokio::test]
async fn concurrent_note_updates_accept_only_one_expected_revision() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("database");
    let note_id = NoteId::new(
        EntityId::from_str("0197c9bc-0000-7000-8000-000000000012").expect("v7 note ID"),
    );
    let owner_identity =
        Identity::new("https://id.example.test".into(), "owner".into()).expect("owner");
    let note = Note::restore(
        note_id,
        owner_identity.clone(),
        "Title".into(),
        "Body".into(),
        Vec::new(),
        UnixMillis::new(100),
        UnixMillis::new(100),
        Revision::INITIAL,
        None,
    )
    .expect("note");
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create");
    let owner = Actor::new(owner_identity);
    let first_draft = NoteDraft {
        title: "First".into(),
        source: "= First\n".into(),
        tags: Vec::new(),
    };
    let second_draft = NoteDraft {
        title: "Second".into(),
        source: "= Second\n".into(),
        tags: Vec::new(),
    };
    let first = database.update_visible_note(
        &owner,
        note_id,
        Revision::INITIAL,
        &first_draft,
        NoteLinks::default(),
        UnixMillis::new(110),
    );
    let second = database.update_visible_note(
        &owner,
        note_id,
        Revision::INITIAL,
        &second_draft,
        NoteLinks::default(),
        UnixMillis::new(120),
    );
    let results = tokio::join!(first, second);
    let successes = [&results.0, &results.1]
        .into_iter()
        .filter(|result| result.is_ok())
        .count();
    let conflicts = [&results.0, &results.1]
        .into_iter()
        .filter(|result| **result == Err(SqliteStoreError::Conflict))
        .count();
    assert_eq!((successes, conflicts), (1, 1));

    let current = database
        .visible_note(&owner, note_id)
        .await
        .expect("read after conflict")
        .expect("visible note");
    let retried = database
        .update_visible_note(
            &owner,
            note_id,
            current.revision(),
            &second_draft,
            NoteLinks::default(),
            UnixMillis::new(130),
        )
        .await
        .expect("retry after conflict");
    assert_eq!(retried.revision().get(), 3);
}

/// 図に出す点と線は、閲覧できるノートと、そこから引用された文献だけとする。
#[tokio::test]
async fn the_graph_hides_notes_and_edges_the_actor_cannot_see() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("schema initialization");
    let alice = actor("https://id.example.test", "alice");
    let bob = actor("https://id.example.test", "bob");

    let shared = graph_note("0197c9bc-0000-7000-8000-000000000001", "共有するノート");
    let private = graph_note("0197c9bc-0000-7000-8000-000000000002", "共有しないノート");
    database
        .create_note(
            &shared,
            NoteLinks {
                reference_targets: &[private.note_id()],
                cited_keys: &["smith2024".to_owned()],
            },
        )
        .await
        .expect("create shared note");
    database
        .create_note(
            &private,
            NoteLinks {
                reference_targets: &[shared.note_id()],
                cited_keys: &["tanaka2025".to_owned()],
            },
        )
        .await
        .expect("create private note");
    database
        .replace_note_acl(
            &alice,
            shared.note_id(),
            &[NoteAclEntry::new(
                Identity::new("https://id.example.test".into(), "bob".into()).expect("identity"),
                NotePermission::Read,
            )],
            Revision::INITIAL,
            UnixMillis::new(200),
        )
        .await
        .expect("share the note with bob");
    database
        .create_owned_item(&BibliographyItem::create(
            BibliographyItemId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-0000000000a1").expect("v7 item ID"),
            ),
            alice.identity(),
            "smith2024".into(),
            r#"{"id":"smith2024","type":"book","title":"An Example"}"#.into(),
            UnixMillis::new(100),
        ))
        .await
        .expect("register the cited work");

    // 作成者は両方のノートと、その間の線を見る。
    let owner_graph = database
        .note_graph(&alice, &NoteGraphQuery::default())
        .await
        .expect("graph for the owner");
    assert_eq!(owner_graph.notes.len(), 2);
    assert_eq!(owner_graph.references.len(), 2);
    assert_eq!(owner_graph.citations.len(), 2);

    // 共有相手は共有されたノートだけを見る。共有していないノートも、そこへ向かう線も現れない。
    let reader_graph = database
        .note_graph(&bob, &NoteGraphQuery::default())
        .await
        .expect("graph for the reader");
    assert_eq!(reader_graph.notes.len(), 1);
    assert_eq!(reader_graph.notes[0].title, "共有するノート");
    assert!(reader_graph.references.is_empty());
    assert_eq!(reader_graph.citations.len(), 1);
    // 引用された文献だけが出る。作成者のライブラリーで解決できた題名を添える。
    assert_eq!(reader_graph.works.len(), 1);
    assert_eq!(reader_graph.works[0].citation_key, "smith2024");
    assert_eq!(reader_graph.works[0].title.as_deref(), Some("An Example"));

    // 語で絞り込むと、その語を持つノートと、そこからの引用だけが残る。
    let filtered = database
        .note_graph(
            &alice,
            &NoteGraphQuery {
                text: Some("共有する".into()),
                ..NoteGraphQuery::default()
            },
        )
        .await
        .expect("filtered graph");
    assert_eq!(filtered.notes.len(), 1);
    assert!(filtered.references.is_empty());
    assert_eq!(filtered.citations.len(), 1);
    assert_eq!(filtered.works.len(), 1);
}

/// 想定規模（REQ-OPS-003の約1,000ノート）で、図の問い合わせが一度で全体を返すことを確かめる。
///
/// 点と線の本数を数えるだけの試験である。所要時間は環境で変わるため上限を判定しない。
#[tokio::test]
async fn the_graph_answers_at_the_assumed_scale() {
    const NOTES: usize = 1_000;
    const WORKS: usize = 50;

    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("schema initialization");
    let alice = actor("https://id.example.test", "alice");
    let identifiers: Vec<NoteId> = (0..NOTES)
        .map(|index| {
            NoteId::new(
                EntityId::from_str(&format!("0197c9bc-0000-7000-8000-{index:012x}"))
                    .expect("v7 note ID"),
            )
        })
        .collect();

    for (index, note_id) in identifiers.iter().enumerate() {
        // 鎖状につなぎ、参照の線が確実に1本ずつ増えるようにする。
        let targets = if index + 1 < NOTES {
            vec![identifiers[index + 1]]
        } else {
            Vec::new()
        };
        let cited = vec![format!("work{:04}", index % WORKS)];
        let note = Note::restore(
            *note_id,
            Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner"),
            format!("規模の確認 {index}"),
            format!(
                "= 規模の確認 {index}\n\n本文と cite:work{:04}[]",
                index % WORKS
            ),
            // 半数へタグを付け、語での絞り込みがタグにも効くことを見る。
            if index % 2 == 0 {
                vec!["調査".to_owned()]
            } else {
                Vec::new()
            },
            UnixMillis::new(100),
            UnixMillis::new(100),
            Revision::INITIAL,
            None,
        )
        .expect("consistent note");
        database
            .create_note(
                &note,
                NoteLinks {
                    reference_targets: &targets,
                    cited_keys: &cited,
                },
            )
            .await
            .expect("create note");
    }

    let whole = database
        .note_graph(&alice, &NoteGraphQuery::default())
        .await
        .expect("graph at scale");
    assert_eq!(whole.notes.len(), NOTES);
    assert_eq!(whole.references.len(), NOTES - 1);
    assert_eq!(whole.citations.len(), NOTES);
    assert_eq!(whole.works.len(), WORKS);

    // タグで絞ると半数になり、線は両端が残る組だけになる。鎖は1つ飛ばしで切れるため0本である。
    let tagged = database
        .note_graph(
            &alice,
            &NoteGraphQuery {
                text: Some("調査".into()),
                ..NoteGraphQuery::default()
            },
        )
        .await
        .expect("filtered graph at scale");
    assert_eq!(tagged.notes.len(), NOTES / 2);
    assert!(tagged.references.is_empty());
    assert_eq!(tagged.citations.len(), NOTES / 2);
}

fn graph_note(id: &str, title: &str) -> Note {
    Note::restore(
        NoteId::new(EntityId::from_str(id).expect("v7 note ID")),
        Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner"),
        title.into(),
        format!("= {title}\n\n本文"),
        Vec::new(),
        UnixMillis::new(100),
        UnixMillis::new(100),
        Revision::INITIAL,
        None,
    )
    .expect("consistent note")
}
