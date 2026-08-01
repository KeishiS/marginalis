use super::*;

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

/// citation keyでの読み取りは、指定した所有者のライブラリーだけを見る。
///
/// 引用の解決はノート作成者のライブラリーを使うため、他の利用者の項目が混ざらないことを
/// 保存側で確かめる。
#[tokio::test]
async fn citation_keys_are_read_only_from_the_named_owner() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("schema initialization");
    let alice = actor("https://id.example.test", "alice");
    let bob = actor("https://id.example.test", "bob");
    let alice_item = BibliographyItem::create(
        BibliographyItemId::new(
            EntityId::from_str("0197c9bc-0000-7000-8000-000000000094").expect("v7 item ID"),
        ),
        alice.identity(),
        "smith2024".into(),
        r#"{"id":"smith2024","type":"article-journal","title":"Alice の登録"}"#.into(),
        UnixMillis::new(100),
    );
    let bob_item = BibliographyItem::create(
        BibliographyItemId::new(
            EntityId::from_str("0197c9bc-0000-7000-8000-000000000095").expect("v7 item ID"),
        ),
        bob.identity(),
        "tanaka2025".into(),
        r#"{"id":"tanaka2025","type":"book","title":"Bob の登録"}"#.into(),
        UnixMillis::new(100),
    );
    for item in [&alice_item, &bob_item] {
        database
            .create_owned_item(item)
            .await
            .expect("create bibliography item");
    }

    let keys = ["smith2024".to_owned(), "tanaka2025".to_owned()];
    assert_eq!(
        database
            .items_by_citation_keys(alice.identity(), &keys)
            .await
            .expect("owner lookup"),
        vec![alice_item]
    );
    assert!(
        database
            .items_by_citation_keys(alice.identity(), &[])
            .await
            .expect("empty lookup")
            .is_empty()
    );
    assert!(
        database
            .items_by_citation_keys(alice.identity(), &["unknown".to_owned()])
            .await
            .expect("unknown key lookup")
            .is_empty()
    );
}

#[tokio::test]
async fn bibliography_search_treats_like_metacharacters_as_text() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("schema initialization");
    let alice = actor("https://id.example.test", "alice");
    for (id, key) in [
        ("0197c9bc-0000-7000-8000-000000000096", "rate%_literal"),
        ("0197c9bc-0000-7000-8000-000000000097", "ordinary"),
    ] {
        let item = BibliographyItem::create(
            BibliographyItemId::new(EntityId::from_str(id).expect("v7 item ID")),
            alice.identity(),
            key.into(),
            format!(r#"{{"id":"{key}","type":"book"}}"#),
            UnixMillis::new(100),
        );
        database
            .create_owned_item(&item)
            .await
            .expect("create bibliography item");
    }

    let percent = database
        .search_owned_items(&alice, "%")
        .await
        .expect("literal percent search");
    assert_eq!(
        percent
            .iter()
            .map(BibliographyItem::citation_key)
            .collect::<Vec<_>>(),
        vec!["rate%_literal"]
    );
    let underscore = database
        .search_owned_items(&alice, "_")
        .await
        .expect("literal underscore search");
    assert_eq!(underscore.len(), 1);
}
