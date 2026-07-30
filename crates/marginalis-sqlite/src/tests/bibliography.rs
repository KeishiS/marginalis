#[tokio::test]
async fn bibliography_is_private_unique_and_revision_guarded() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("schema initialization");
    let alice = actor("https://id.example.test", "alice");
    let bob = actor("https://id.example.test", "bob");
    let item_id = BibliographyItemId::new(
        EntityId::from_str("0197c9bc-0000-7000-8000-000000000091").expect("v7 item ID"),
    );
    let item = BibliographyItem::create(
        item_id,
        alice.identity(),
        "smith2024".into(),
        r#"{"id":"smith2024","type":"article-journal","title":"Example"}"#.into(),
        UnixMillis::new(100),
    );
    database
        .create_owned_item(&item)
        .await
        .expect("create bibliography item");

    assert_eq!(
        database
            .search_owned_items(&alice, "Example")
            .await
            .expect("owner search"),
        vec![item.clone()]
    );
    assert!(
        database
            .search_owned_items(&bob, "")
            .await
            .expect("other user search")
            .is_empty()
    );
    assert_eq!(
        database.create_owned_item(&item).await,
        Err(BibliographyRepositoryError::Conflict)
    );
    assert_eq!(
        database
            .update_owned_item(
                &bob,
                item_id,
                "smith2025",
                r#"{"id":"smith2025","type":"book"}"#,
                UnixMillis::new(200),
                Revision::INITIAL,
            )
            .await,
        Err(BibliographyRepositoryError::NotFound)
    );
    let updated = database
        .update_owned_item(
            &alice,
            item_id,
            "smith2025",
            r#"{"id":"smith2025","type":"book"}"#,
            UnixMillis::new(200),
            Revision::INITIAL,
        )
        .await
        .expect("update bibliography item");
    assert_eq!(updated.citation_key(), "smith2025");
    assert_eq!(updated.revision(), revision(2));
    assert_eq!(
        database
            .delete_owned_item(&alice, item_id, Revision::INITIAL)
            .await,
        Err(BibliographyRepositoryError::Conflict)
    );
    assert_eq!(
        database
            .delete_owned_item(&bob, item_id, Revision::INITIAL)
            .await,
        Err(BibliographyRepositoryError::NotFound)
    );
    database
        .delete_owned_item(&alice, item_id, revision(2))
        .await
        .expect("delete bibliography item");
    assert!(
        database
            .search_owned_items(&alice, "")
            .await
            .expect("empty owner library")
            .is_empty()
    );
}
