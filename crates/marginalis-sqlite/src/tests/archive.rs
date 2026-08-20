use marginalis_application::RestorePlan;

use super::*;

#[tokio::test]
async fn archive_snapshot_restores_primary_identities_aliases_and_empty_principals() {
    let source = database().await;
    let alice = user("alice");
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
        target
            .export_archive_snapshot()
            .await
            .expect("restored snapshot"),
        snapshot
    );
}
