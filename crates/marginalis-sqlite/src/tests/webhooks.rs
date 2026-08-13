use marginalis_application::{
    BibliographyImportCommit, BibliographyImportItemMutation, BibliographyImportState, NoteLinks,
};
use marginalis_domain::{
    BibliographyContentDigest, BibliographyImportLink, BibliographyImportSource,
    BibliographyImportSourceId,
};

use super::*;

/// outboxの行を発生順に読む。(event_kind, target_id, revision, event_id)
async fn outbox_rows(database: &SqliteDatabase) -> Vec<(String, String, i64, String)> {
    sqlx::query_as::<_, (String, String, i64, String)>(
        "SELECT event_kind, target_id, revision, event_id
         FROM webhook_outbox_events ORDER BY event_sequence",
    )
    .fetch_all(&database.pool)
    .await
    .expect("outbox query")
}

/// 試験用のsubscription行を直接挿入する(管理APIは後続issueで実装する)。
async fn insert_subscription(
    database: &SqliteDatabase,
    subscription_id: &str,
    subject: &str,
    state: &str,
    event_kinds_json: &str,
) {
    sqlx::query(
        "INSERT INTO webhook_subscriptions (
             subscription_id, owner_issuer, owner_subject, url, secret,
             event_kinds_json, state, disabled_reason,
             created_at_ms, updated_at_ms, revision
         ) VALUES (?, ?, ?, 'https://receiver.example.test/hook', 'test-secret',
                   ?, ?, NULL, 0, 0, 1)",
    )
    .bind(subscription_id)
    .bind(ISSUER)
    .bind(subject)
    .bind(event_kinds_json)
    .bind(state)
    .execute(&database.pool)
    .await
    .expect("insert subscription");
}

/// ノートの作成、本文更新、削除、復元だけがeventになり、ACLと人手確認の更新、
/// 確定しなかった操作(権限なし・競合)ではeventが生まれない。
#[tokio::test]
async fn note_lifecycle_emits_exactly_one_event_per_confirmed_change() {
    let database = database().await;
    let alice = user("alice");
    let bob = user("bob");
    let charlie = user("charlie");
    let id = note_id("0197c9bc-0000-7000-8000-0000000000e1");
    let note = note_seed("0197c9bc-0000-7000-8000-0000000000e1", "alice", "Title")
        .source("body")
        .build();
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create note");

    // 権限のない更新と競合した更新は確定しないため、eventも増えない。
    assert_eq!(
        database
            .update_visible_note(
                &charlie,
                id,
                revision(1),
                &draft("Denied", "= Denied\n\ndenied", &[]),
                NoteLinks::default(),
                UnixMillis::new(150),
            )
            .await,
        Err(SqliteStoreError::NotFound)
    );
    database
        .update_visible_note(
            &alice,
            id,
            revision(1),
            &draft("Updated", "= Updated\n\nupdated body", &[]),
            NoteLinks::default(),
            UnixMillis::new(200),
        )
        .await
        .expect("update note");
    assert_eq!(
        database
            .update_visible_note(
                &alice,
                id,
                revision(1),
                &draft("Stale", "= Stale\n\nstale", &[]),
                NoteLinks::default(),
                UnixMillis::new(210),
            )
            .await,
        Err(SqliteStoreError::Conflict)
    );

    // ACLと人手確認はrevisionを進めるが、通知対象ではない。
    database
        .replace_note_acl(
            &alice,
            id,
            &[NoteAclEntry::new(
                bob.identity().clone(),
                NotePermission::Edit,
            )],
            revision(2),
            UnixMillis::new(250),
        )
        .await
        .expect("replace acl");
    database
        .mark_owned_note_reviewed(&alice, id, revision(3), UnixMillis::new(260))
        .await
        .expect("mark reviewed");

    // 共有先の編集者による本文更新も、所有者のeventとして記録される。
    database
        .update_visible_note(
            &bob,
            id,
            revision(4),
            &draft("Edited by bob", "= Edited\n\nedited by bob", &[]),
            NoteLinks::default(),
            UnixMillis::new(300),
        )
        .await
        .expect("editor update");
    database
        .soft_delete_visible_note(&alice, id, revision(5), UnixMillis::new(400))
        .await
        .expect("soft delete");
    database
        .restore_owned_deleted_note(&alice, id, revision(6), UnixMillis::new(500))
        .await
        .expect("restore");

    let rows = outbox_rows(&database).await;
    let kinds: Vec<&str> = rows.iter().map(|row| row.0.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "note.created",
            "note.updated",
            "note.updated",
            "note.deleted",
            "note.restored",
        ]
    );
    assert!(rows.iter().all(|row| row.1 == id.to_string()));
    assert_eq!(
        rows.iter().map(|row| row.2).collect::<Vec<_>>(),
        [1, 2, 5, 6, 7]
    );
    // event IDは重複しない。
    let mut ids: Vec<&str> = rows.iter().map(|row| row.3.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), rows.len());
    // 所有者はノート作成者のまま変わらない。
    let owners = sqlx::query_as::<_, (String, String)>(
        "SELECT DISTINCT owner_issuer, owner_subject FROM webhook_outbox_events",
    )
    .fetch_all(&database.pool)
    .await
    .expect("owner query");
    assert_eq!(owners, vec![(ISSUER.to_string(), "alice".to_string())]);
}

/// 書誌項目の直接操作と一括取込が、確定した作成・更新・削除ごとに1件のeventになる。
#[tokio::test]
async fn bibliography_changes_emit_events_for_direct_and_bulk_operations() {
    let database = database().await;
    let alice = actor(ISSUER, "alice");
    let item_id = BibliographyItemId::new(
        EntityId::from_str("0197c9bc-0000-7000-8000-0000000000f1").expect("v7 item ID"),
    );
    let item = BibliographyItem::create(
        item_id,
        alice.identity(),
        validated_csl_json(
            "smith2024",
            r#"{"id":"smith2024","type":"article-journal","title":"Example"}"#,
        ),
        UnixMillis::new(100),
    );
    database
        .create_owned_item(&item)
        .await
        .expect("create item");
    database
        .update_owned_item(
            &alice,
            item_id,
            &validated_csl_json("smith2025", r#"{"id":"smith2025","type":"book"}"#),
            UnixMillis::new(200),
            Revision::INITIAL,
        )
        .await
        .expect("update item");
    database
        .delete_owned_item(&alice, item_id, revision(2))
        .await
        .expect("delete item");

    // 一括取込は実際に作成した項目ごとに1件生成される。
    let imported = BibliographyItem::create(
        BibliographyItemId::new(
            EntityId::from_str("0197c9bc-0000-7000-8000-0000000000f2").expect("v7 item ID"),
        ),
        alice.identity(),
        validated_csl_json(
            "tanaka2025",
            r#"{"id":"tanaka2025","type":"book","title":"Imported"}"#,
        ),
        UnixMillis::new(300),
    );
    database
        .apply_import(
            &alice,
            BibliographyImportCommit {
                source: BibliographyImportSource::create(
                    BibliographyImportSourceId::new(
                        EntityId::from_str("0197c9bc-0000-7000-8000-0000000000f3")
                            .expect("v7 source ID"),
                    ),
                    alice.identity(),
                    "Zotero".into(),
                    UnixMillis::new(300),
                )
                .expect("import source"),
                expected_state: BibliographyImportState {
                    source: None,
                    links: Vec::new(),
                    items: Vec::new(),
                },
                imported_at: UnixMillis::new(300),
                mutations: vec![BibliographyImportItemMutation::Create {
                    link: BibliographyImportLink::new(
                        BibliographyImportSourceId::new(
                            EntityId::from_str("0197c9bc-0000-7000-8000-0000000000f3")
                                .expect("v7 source ID"),
                        ),
                        "tanaka2025".into(),
                        imported.item_id(),
                        BibliographyContentDigest::new([7; 32]),
                        Revision::INITIAL,
                    )
                    .expect("link"),
                    item: imported.clone(),
                }],
                excluded: 0,
            },
        )
        .await
        .expect("apply import");

    let rows = outbox_rows(&database).await;
    let kinds: Vec<&str> = rows.iter().map(|row| row.0.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "bibliography_item.created",
            "bibliography_item.updated",
            "bibliography_item.deleted",
            "bibliography_item.created",
        ]
    );
    // 削除はその時点のrevisionを保つ。
    assert_eq!(rows[2].2, 2);
    assert_eq!(rows[3].1, imported.item_id().to_string());
}

/// eventは発生時に有効な購読へだけ展開され、他の利用者や無効な購読には届かない。
#[tokio::test]
async fn fan_out_targets_only_matching_active_subscriptions() {
    let database = database().await;
    insert_subscription(
        &database,
        "sub-active",
        "alice",
        "active",
        r#"["note.created"]"#,
    )
    .await;
    insert_subscription(
        &database,
        "sub-other-kind",
        "alice",
        "active",
        r#"["bibliography_item.created"]"#,
    )
    .await;
    insert_subscription(
        &database,
        "sub-pending",
        "alice",
        "pending_challenge",
        r#"["note.created"]"#,
    )
    .await;
    insert_subscription(&database, "sub-bob", "bob", "active", r#"["note.created"]"#).await;

    let note = note_seed("0197c9bc-0000-7000-8000-0000000000e2", "alice", "Title")
        .source("body")
        .build();
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create note");

    let deliveries = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT subscription_id, state, attempt_count FROM webhook_deliveries",
    )
    .fetch_all(&database.pool)
    .await
    .expect("delivery query");
    assert_eq!(
        deliveries,
        vec![("sub-active".to_string(), "pending".to_string(), 0)]
    );
}
