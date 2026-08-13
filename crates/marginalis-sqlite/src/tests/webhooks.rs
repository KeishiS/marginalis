use marginalis_application::{
    BibliographyImportCommit, BibliographyImportItemMutation, BibliographyImportState, NoteLinks,
    WebhookDeliveryFailure, WebhookDeliveryRepository, WebhookSubscriptionRepository,
    WebhookSubscriptionState,
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

/// 取得は期限とleaseを尊重し、同じsubscriptionでは順序の先頭だけを占有する。
#[tokio::test]
async fn claiming_respects_order_lease_and_subscription_state() {
    let database = database().await;
    insert_subscription(
        &database,
        "sub-a",
        "alice",
        "active",
        r#"["note.created","note.updated"]"#,
    )
    .await;
    let first = note_seed("0197c9bc-0000-7000-8000-0000000000e3", "alice", "One")
        .source("one")
        .build();
    database
        .create_note(&first, NoteLinks::default())
        .await
        .expect("create first note");
    database
        .update_visible_note(
            &user("alice"),
            note_id("0197c9bc-0000-7000-8000-0000000000e3"),
            revision(1),
            &draft("Two", "= Two\n\ntwo", &[]),
            NoteLinks::default(),
            UnixMillis::new(200),
        )
        .await
        .expect("update note");

    // 2件が配送待ちでも、先頭の1件だけが取得される。
    let now = UnixMillis::new(1_000);
    let lease_until = UnixMillis::new(61_000);
    let claimed = database
        .claim_due_deliveries(now, lease_until, 10)
        .await
        .expect("claim head");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].event.kind, "note.created");
    assert_eq!(claimed[0].attempt_count, 0);

    // lease中は同じ配送を再取得できない。
    assert!(
        database
            .claim_due_deliveries(now, lease_until, 10)
            .await
            .expect("claim during lease")
            .is_empty()
    );

    // lease期限が切れると同じ先頭を再取得できる(異常終了からの引き継ぎ)。
    let after_lease = UnixMillis::new(62_000);
    let reclaimed = database
        .claim_due_deliveries(after_lease, UnixMillis::new(122_000), 10)
        .await
        .expect("claim after lease expiry");
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].event.kind, "note.created");

    // 失敗の記録でleaseが解け、次回試行まで取得されない。
    database
        .record_failed(
            "sub-a",
            reclaimed[0].event.sequence,
            WebhookDeliveryFailure::ConnectFailed,
            1,
            UnixMillis::new(200_000),
            after_lease,
        )
        .await
        .expect("record failure");
    assert!(
        database
            .claim_due_deliveries(UnixMillis::new(150_000), UnixMillis::new(210_000), 10)
            .await
            .expect("claim before next attempt")
            .is_empty()
    );

    // 先頭を配送済みにすると、次のeventが取得できるようになる。
    database
        .record_delivered(
            "sub-a",
            reclaimed[0].event.sequence,
            UnixMillis::new(210_000),
        )
        .await
        .expect("record delivered");
    let next = database
        .claim_due_deliveries(UnixMillis::new(220_000), UnixMillis::new(280_000), 10)
        .await
        .expect("claim next event");
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].event.kind, "note.updated");

    // subscriptionを無効化すると、残りの配送は取得されない。
    database
        .disable_exhausted_subscription("sub-a", UnixMillis::new(230_000))
        .await
        .expect("disable subscription");
    assert!(
        database
            .claim_due_deliveries(UnixMillis::new(300_000), UnixMillis::new(360_000), 10)
            .await
            .expect("claim after disable")
            .is_empty()
    );
    let (state, reason) = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT state, disabled_reason FROM webhook_subscriptions WHERE subscription_id = 'sub-a'",
    )
    .fetch_one(&database.pool)
    .await
    .expect("subscription state");
    assert_eq!(
        (state.as_str(), reason.as_deref()),
        ("disabled", Some("delivery_exhausted"))
    );
}

/// 保持期間を過ぎた配送済みeventは消え、未配送が残るeventは保持される。
#[tokio::test]
async fn purging_keeps_undelivered_events_and_removes_expired_ones() {
    let database = database().await;
    insert_subscription(&database, "sub-a", "alice", "active", r#"["note.created"]"#).await;
    let note = note_seed("0197c9bc-0000-7000-8000-0000000000e4", "alice", "Kept")
        .source("kept")
        .build();
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create note");
    let claimed = database
        .claim_due_deliveries(UnixMillis::new(1_000), UnixMillis::new(61_000), 10)
        .await
        .expect("claim");
    // 1件目: 配送済みとして古い時刻を記録し、保持期限切れにする。
    database
        .record_delivered("sub-a", claimed[0].event.sequence, UnixMillis::new(2_000))
        .await
        .expect("deliver");

    // 2件目: 未配送のまま古いeventとして残す。
    let pending = note_seed("0197c9bc-0000-7000-8000-0000000000e5", "alice", "Pending")
        .source("pending")
        .build();
    database
        .create_note(&pending, NoteLinks::default())
        .await
        .expect("create pending note");

    let far_future = UnixMillis::new(2_000 + 8 * 24 * 60 * 60 * 1000);
    let removed = database
        .purge_expired_events(far_future)
        .await
        .expect("purge");
    assert_eq!(removed, 1);
    let remaining = outbox_rows(&database).await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].1, pending.note_id().to_string());
}

/// 購読の管理は所有者だけへ働き、一覧は配送状況の要約を含む。
#[tokio::test]
async fn subscription_management_is_scoped_to_the_owner() {
    let database = database().await;
    let alice = user("alice");
    let bob = user("bob");
    database
        .create_owned_subscription(
            &alice,
            "0197c9bc-0000-7000-8000-0000000000f1",
            "https://receiver.example.test/hook",
            &["note.created".to_string()],
            "secret-1",
            UnixMillis::new(1_000),
        )
        .await
        .expect("create subscription");

    // 一覧と資格情報は所有者だけが読める。
    let owned = database
        .list_owned_subscriptions(&alice)
        .await
        .expect("list owned");
    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0].state, WebhookSubscriptionState::PendingChallenge);
    assert_eq!(owned[0].event_kinds, vec!["note.created".to_string()]);
    assert_eq!(owned[0].pending_count, 0);
    assert!(
        database
            .list_owned_subscriptions(&bob)
            .await
            .expect("list bob")
            .is_empty()
    );
    assert_eq!(
        database
            .owned_subscription_credentials(&bob, "0197c9bc-0000-7000-8000-0000000000f1")
            .await
            .expect("credentials for bob"),
        None
    );

    // 有効化とsecretの更新はrevisionを進める。
    assert!(
        database
            .activate_owned_subscription(
                &alice,
                "0197c9bc-0000-7000-8000-0000000000f1",
                UnixMillis::new(2_000),
            )
            .await
            .expect("activate")
    );
    assert!(
        database
            .replace_owned_secret(
                &alice,
                "0197c9bc-0000-7000-8000-0000000000f1",
                "secret-2",
                UnixMillis::new(3_000),
            )
            .await
            .expect("replace secret")
    );
    let credentials = database
        .owned_subscription_credentials(&alice, "0197c9bc-0000-7000-8000-0000000000f1")
        .await
        .expect("credentials");
    assert_eq!(
        credentials,
        Some((
            "https://receiver.example.test/hook".to_string(),
            "secret-2".to_string()
        ))
    );
    let owned = database
        .list_owned_subscriptions(&alice)
        .await
        .expect("list after updates");
    assert_eq!(owned[0].state, WebhookSubscriptionState::Active);
    assert_eq!(owned[0].revision, 3);

    // 削除も所有者だけができ、削除後は見えない。
    assert!(
        !database
            .delete_owned_subscription(&bob, "0197c9bc-0000-7000-8000-0000000000f1")
            .await
            .expect("delete by bob")
    );
    assert!(
        database
            .delete_owned_subscription(&alice, "0197c9bc-0000-7000-8000-0000000000f1")
            .await
            .expect("delete by alice")
    );
    assert!(
        database
            .list_owned_subscriptions(&alice)
            .await
            .expect("list after delete")
            .is_empty()
    );
}

/// 再試行は失敗中の先頭を初期化して停止を解除し、破棄は後続を進める。
#[tokio::test]
async fn retry_and_discard_operate_on_the_head_delivery() {
    let database = database().await;
    let alice = user("alice");
    insert_subscription(
        &database,
        "sub-a",
        "alice",
        "active",
        r#"["note.created","note.updated"]"#,
    )
    .await;
    let note = note_seed("0197c9bc-0000-7000-8000-0000000000f2", "alice", "One")
        .source("one")
        .build();
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create note");
    database
        .update_visible_note(
            &alice,
            note_id("0197c9bc-0000-7000-8000-0000000000f2"),
            revision(1),
            &draft("Two", "= Two\n\ntwo", &[]),
            NoteLinks::default(),
            UnixMillis::new(200),
        )
        .await
        .expect("update note");

    // 先頭を試行上限まで失敗させ、subscriptionを無効化した状態を作る。
    let claimed = database
        .claim_due_deliveries(UnixMillis::new(1_000), UnixMillis::new(61_000), 10)
        .await
        .expect("claim head");
    database
        .record_failed(
            "sub-a",
            claimed[0].event.sequence,
            WebhookDeliveryFailure::TimedOut,
            10,
            UnixMillis::new(3_600_000),
            UnixMillis::new(1_500),
        )
        .await
        .expect("record failure");
    database
        .disable_exhausted_subscription("sub-a", UnixMillis::new(1_600))
        .await
        .expect("disable");

    // 一覧は停止と失敗分類、配送待ち2件を示す。
    let owned = database
        .list_owned_subscriptions(&alice)
        .await
        .expect("list disabled");
    assert_eq!(owned[0].state, WebhookSubscriptionState::Disabled);
    assert_eq!(
        owned[0].disabled_reason.as_deref(),
        Some("delivery_exhausted")
    );
    assert_eq!(owned[0].last_failure.as_deref(), Some("timed_out"));
    assert_eq!(owned[0].pending_count, 2);

    // 再試行は先頭の試行回数を初期化し、購読を有効へ戻す。
    assert!(
        database
            .retry_owned_head_delivery(&alice, "sub-a", UnixMillis::new(2_000))
            .await
            .expect("retry")
    );
    let reclaimed = database
        .claim_due_deliveries(UnixMillis::new(2_500), UnixMillis::new(62_500), 10)
        .await
        .expect("claim after retry");
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].event.kind, "note.created");
    assert_eq!(reclaimed[0].attempt_count, 0);

    // 破棄は先頭を取り除き、後続のeventが取得できるようになる。
    assert!(
        database
            .discard_owned_head_delivery(&alice, "sub-a", UnixMillis::new(3_000))
            .await
            .expect("discard")
    );
    let next = database
        .claim_due_deliveries(UnixMillis::new(3_500), UnixMillis::new(63_500), 10)
        .await
        .expect("claim after discard");
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].event.kind, "note.updated");

    // 所有していない利用者の操作は対象なしとして拒否する。
    assert!(
        !database
            .retry_owned_head_delivery(&user("bob"), "sub-a", UnixMillis::new(4_000))
            .await
            .expect("retry by bob")
    );
}
