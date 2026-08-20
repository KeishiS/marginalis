use marginalis_application::RestorePlan;
use marginalis_domain::{AttachmentDraft, AttachmentId};

use super::*;

#[tokio::test]
async fn archive_snapshot_restores_primary_identities_aliases_and_empty_principals() {
    let source = database().await;
    let alice = user("alice");
    let note = note_seed(
        "0197c9bc-0000-7000-8000-000000000091",
        "alice",
        "archive history",
    )
    .build();
    source
        .create_note(&note, marginalis_application::NoteLinks::default())
        .await
        .expect("create note");
    let attachment_id = "0197c9bc-0000-7000-8000-0000000000a1"
        .parse::<AttachmentId>()
        .expect("attachment ID");
    let attachment = AttachmentDraft::new(
        "figure.png".into(),
        b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01payload".to_vec(),
    )
    .expect("image")
    .into_stored(
        attachment_id,
        note.note_id(),
        UnixMillis::new(150),
        alice.principal().clone(),
    );
    source
        .create_note_attachment(&alice, &attachment)
        .await
        .expect("store attachment");
    source
        .update_visible_note(
            &alice,
            note.note_id(),
            Revision::INITIAL,
            &draft(
                "archive history",
                &format!("= archive history\n\nsecond\n\nimage::attachment:{attachment_id}[]"),
                &[],
            ),
            marginalis_application::NoteLinks {
                attachment_ids: &[attachment_id],
                ..marginalis_application::NoteLinks::default()
            },
            UnixMillis::new(200),
        )
        .await
        .expect("update note");
    let alias = Identity::new(
        "https://replacement-id.example.test".into(),
        "alice-after-migration".into(),
    )
    .expect("alias identity");
    let mut transaction = source.pool.begin().await.expect("transaction");
    sqlx::query(
        "INSERT INTO principal_identities (principal_id, issuer, subject, is_primary)
         VALUES (?, ?, ?, 0)",
    )
    .bind(alice.principal_id().get())
    .bind(alias.issuer())
    .bind(alias.subject())
    .execute(&mut *transaction)
    .await
    .expect("alias");
    sqlx::query("UPDATE principal_identities SET is_primary = 0 WHERE principal_id = ?")
        .bind(alice.principal_id().get())
        .execute(&mut *transaction)
        .await
        .expect("clear primary");
    sqlx::query(
        "UPDATE principal_identities SET is_primary = 1
         WHERE principal_id = ? AND issuer = ? AND subject = ?",
    )
    .bind(alice.principal_id().get())
    .bind(alias.issuer())
    .bind(alias.subject())
    .execute(&mut *transaction)
    .await
    .expect("new primary");
    transaction.commit().await.expect("commit identity change");

    let snapshot = source.export_archive_snapshot().await.expect("snapshot");
    assert_eq!(snapshot.note_revisions().len(), 2);
    assert_eq!(snapshot.attachments().len(), 1);
    assert_eq!(snapshot.note_revision_attachments().len(), 1);
    let alice_group = snapshot
        .principals()
        .iter()
        .find(|principal| principal.id() == alice.principal_id())
        .expect("alice principal");
    assert_eq!(alice_group.primary_identity(), &alias);
    assert!(alice_group.contains(alice.authenticated_identity()));
    assert_eq!(
        snapshot.principals().len(),
        14,
        "業務データがないprincipalのidentity引き継ぎも災害復旧で失わない"
    );

    let plan = RestorePlan::new(snapshot.clone(), Vec::new(), Vec::new()).expect("restore plan");
    let target = empty_database().await;
    target.restore(&plan).await.expect("restore snapshot");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM note_sync_changes")
            .fetch_one(&target.pool)
            .await
            .expect("sync change count"),
        0,
        "archive復元は検索投影の変更索引を引き継ぎません"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM webhook_outbox_events")
            .fetch_one(&target.pool)
            .await
            .expect("webhook outbox count"),
        0,
        "archive復元中のtriggerが作った通知を残しません"
    );
    assert_eq!(
        target
            .export_archive_snapshot()
            .await
            .expect("restored snapshot"),
        snapshot
    );
}
