use marginalis_application::{
    BibliographyImportCommit, BibliographyImportItemMutation, BibliographyImportRepositoryError,
    BibliographyImportState, RestorePlan,
};
use marginalis_domain::{
    BibliographyContentDigest, BibliographyImportLink, BibliographyImportSource,
    BibliographyImportSourceId,
};

use super::*;

fn source_id() -> BibliographyImportSourceId {
    BibliographyImportSourceId::new(
        EntityId::from_str("0197c9bc-0000-7000-8000-0000000000c1").expect("UUIDv7"),
    )
}

fn item_id() -> BibliographyItemId {
    BibliographyItemId::new(
        EntityId::from_str("0197c9bc-0000-7000-8000-0000000000c2").expect("UUIDv7"),
    )
}

fn source(owner: &Actor) -> BibliographyImportSource {
    BibliographyImportSource::create(
        source_id(),
        owner.identity(),
        "Zotero".into(),
        UnixMillis::new(100),
    )
    .expect("source")
}

fn item(owner: &Actor) -> BibliographyItem {
    BibliographyItem::create(
        item_id(),
        owner.identity(),
        "smith2024".into(),
        r#"{"id":"smith2024","type":"book","title":"Before"}"#.into(),
        UnixMillis::new(100),
    )
}

fn link(external_id: &str, item_revision: Revision) -> BibliographyImportLink {
    BibliographyImportLink::new(
        source_id(),
        external_id.into(),
        item_id(),
        BibliographyContentDigest::new([7; 32]),
        item_revision,
    )
    .expect("link")
}

fn empty_state() -> BibliographyImportState {
    BibliographyImportState {
        source: None,
        links: Vec::new(),
        items: Vec::new(),
    }
}

#[tokio::test]
async fn import_commit_creates_owner_scoped_source_item_and_link() {
    let database = database().await;
    let alice = actor("https://id.example.test", "alice");
    let bob = actor("https://id.example.test", "bob");
    let item = item(&alice);
    let result = database
        .apply_import(
            &alice,
            BibliographyImportCommit {
                source: source(&alice),
                expected_state: empty_state(),
                imported_at: UnixMillis::new(100),
                mutations: vec![BibliographyImportItemMutation::Create {
                    link: link("smith2024", item.revision()),
                    item: item.clone(),
                }],
                excluded: 2,
            },
        )
        .await
        .expect("apply import");
    assert_eq!(result.source_revision, Revision::INITIAL);
    assert_eq!((result.created, result.excluded), (1, 2));

    let state = database
        .load_import_state(&alice, Some(source_id()))
        .await
        .expect("load import state");
    assert_eq!(
        state.source.as_ref().map(|source| source.display_name()),
        Some("Zotero")
    );
    assert_eq!(state.items, vec![item]);
    assert_eq!(state.links, vec![link("smith2024", Revision::INITIAL)]);
    assert!(
        database
            .list_import_sources(&bob)
            .await
            .expect("other owner sources")
            .is_empty()
    );
    assert!(
        database
            .load_import_state(&bob, Some(source_id()))
            .await
            .expect("other owner state")
            .source
            .is_none()
    );
}

#[tokio::test]
async fn revision_conflict_rolls_back_every_item_and_the_source() {
    let database = database().await;
    let alice = actor("https://id.example.test", "alice");
    let original = item(&alice);
    database
        .apply_import(
            &alice,
            BibliographyImportCommit {
                source: source(&alice),
                expected_state: empty_state(),
                imported_at: UnixMillis::new(100),
                mutations: vec![BibliographyImportItemMutation::Create {
                    link: link("smith2024", Revision::INITIAL),
                    item: original.clone(),
                }],
                excluded: 0,
            },
        )
        .await
        .expect("initial import");
    let expected_state = database
        .load_import_state(&alice, Some(source_id()))
        .await
        .expect("state at preview");

    let result = database
        .apply_import(
            &alice,
            BibliographyImportCommit {
                source: source(&alice),
                expected_state,
                imported_at: UnixMillis::new(200),
                mutations: vec![
                    BibliographyImportItemMutation::Update {
                        item_id: item_id(),
                        csl_json: r#"{"id":"smith2024","type":"book","title":"After"}"#.into(),
                        expected_revision: Revision::INITIAL,
                        link: link("smith2024", revision(2)),
                        updated_at: UnixMillis::new(200),
                    },
                    BibliographyImportItemMutation::Keep {
                        expected_revision: revision(99),
                        link: link("second-external-id", revision(99)),
                    },
                ],
                excluded: 0,
            },
        )
        .await;
    assert_eq!(result, Err(BibliographyImportRepositoryError::Conflict));

    let state = database
        .load_import_state(&alice, Some(source_id()))
        .await
        .expect("state after rollback");
    assert_eq!(state.source.expect("source").revision(), Revision::INITIAL);
    assert_eq!(state.items, vec![original]);
    assert_eq!(state.links, vec![link("smith2024", Revision::INITIAL)]);
}

#[tokio::test]
async fn an_unrelated_library_change_invalidates_the_whole_preview() {
    let database = database().await;
    let alice = actor("https://id.example.test", "alice");
    let original = item(&alice);
    database
        .apply_import(
            &alice,
            BibliographyImportCommit {
                source: source(&alice),
                expected_state: empty_state(),
                imported_at: UnixMillis::new(100),
                mutations: vec![BibliographyImportItemMutation::Create {
                    link: link("smith2024", Revision::INITIAL),
                    item: original,
                }],
                excluded: 0,
            },
        )
        .await
        .expect("initial import");
    let expected_state = database
        .load_import_state(&alice, Some(source_id()))
        .await
        .expect("state at preview");
    let unrelated = BibliographyItem::create(
        BibliographyItemId::new(
            EntityId::from_str("0197c9bc-0000-7000-8000-0000000000c3").expect("UUIDv7"),
        ),
        alice.identity(),
        "unrelated".into(),
        r#"{"id":"unrelated","type":"book"}"#.into(),
        UnixMillis::new(150),
    );
    database
        .create_owned_item(&unrelated)
        .await
        .expect("concurrent library change");

    let result = database
        .apply_import(
            &alice,
            BibliographyImportCommit {
                source: source(&alice),
                expected_state,
                imported_at: UnixMillis::new(200),
                mutations: vec![BibliographyImportItemMutation::Keep {
                    expected_revision: Revision::INITIAL,
                    link: link("smith2024", Revision::INITIAL),
                }],
                excluded: 0,
            },
        )
        .await;
    assert_eq!(result, Err(BibliographyImportRepositoryError::Conflict));

    let current = database
        .load_import_state(&alice, Some(source_id()))
        .await
        .expect("state after conflict");
    assert_eq!(
        current.source.expect("source").revision(),
        Revision::INITIAL
    );
    assert_eq!(current.items.len(), 2);
}

#[tokio::test]
async fn import_rejects_a_source_owned_by_another_actor() {
    let database = database().await;
    let alice = actor("https://id.example.test", "alice");
    let bob = actor("https://id.example.test", "bob");
    let result = database
        .apply_import(
            &bob,
            BibliographyImportCommit {
                source: source(&alice),
                expected_state: empty_state(),
                imported_at: UnixMillis::new(100),
                mutations: Vec::new(),
                excluded: 0,
            },
        )
        .await;
    assert_eq!(result, Err(BibliographyImportRepositoryError::NotFound));
}

#[tokio::test]
async fn loading_import_state_rejects_a_baseline_newer_than_the_item() {
    let database = database().await;
    let alice = actor("https://id.example.test", "alice");
    let item = item(&alice);
    database
        .apply_import(
            &alice,
            BibliographyImportCommit {
                source: source(&alice),
                expected_state: empty_state(),
                imported_at: UnixMillis::new(100),
                mutations: vec![BibliographyImportItemMutation::Create {
                    item,
                    link: link("external-smith", Revision::INITIAL),
                }],
                excluded: 0,
            },
        )
        .await
        .expect("initial import");
    sqlx::query(
        "UPDATE bibliography_import_links SET imported_item_revision = 2
         WHERE source_id = ? AND external_item_id = 'external-smith'",
    )
    .bind(source_id().to_string())
    .execute(&database.pool)
    .await
    .expect("corrupt baseline");

    assert_eq!(
        database.load_import_state(&alice, Some(source_id())).await,
        Err(BibliographyImportRepositoryError::CorruptData)
    );
}

#[tokio::test]
async fn archive_snapshot_restores_import_sources_links_and_baselines() {
    let database = database().await;
    let alice = actor("https://id.example.test", "alice");
    let item = item(&alice);
    database
        .apply_import(
            &alice,
            BibliographyImportCommit {
                source: source(&alice),
                expected_state: empty_state(),
                imported_at: UnixMillis::new(100),
                mutations: vec![BibliographyImportItemMutation::Create {
                    item,
                    link: link("external-smith", Revision::INITIAL),
                }],
                excluded: 0,
            },
        )
        .await
        .expect("initial import");
    let snapshot = database
        .export_archive_snapshot()
        .await
        .expect("export snapshot");
    assert_eq!(snapshot.bibliography_import_sources().len(), 1);
    assert_eq!(snapshot.bibliography_import_links().len(), 1);

    let restored = super::database().await;
    let plan = RestorePlan::new(snapshot.clone(), Vec::new(), Vec::new()).expect("restore plan");
    restored.restore(&plan).await.expect("restore");
    assert_eq!(
        restored
            .export_archive_snapshot()
            .await
            .expect("re-export snapshot"),
        snapshot
    );
    let state = restored
        .load_import_state(&alice, Some(source_id()))
        .await
        .expect("restored import state");
    assert_eq!(state.links[0].external_item_id(), "external-smith");
    assert_eq!(
        state.links[0].imported_digest(),
        BibliographyContentDigest::new([7; 32])
    );
}
