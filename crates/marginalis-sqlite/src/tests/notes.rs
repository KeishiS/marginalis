use marginalis_application::{NoteGraphQuery, NoteLinks, NoteListQuery};
use marginalis_domain::NoteReviewStatus;

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

fn graph_note(id: &str, title: &str) -> Note {
    note_seed(id, "alice", title).build()
}

#[tokio::test]
async fn every_identity_alias_uses_the_same_owner_and_acl_permissions() {
    let database = database().await;
    let note = note_seed(
        "0197c9bc-0000-7000-8000-000000000099",
        "alice",
        "Alias access",
    )
    .build();
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create note");
    let alice = user("alice");
    let alice_alias = Identity::new(
        "https://replacement-id.example.test".into(),
        "alice-new".into(),
    )
    .expect("alias");
    sqlx::query(
        "INSERT INTO principal_identities (principal_id, issuer, subject, is_primary)
         VALUES (?, ?, ?, 0)",
    )
    .bind(alice.principal_id().get())
    .bind(alice_alias.issuer())
    .bind(alice_alias.subject())
    .execute(&database.pool)
    .await
    .expect("insert alias");
    let alias_actor = marginalis_application::PrincipalDirectory::resolve(&database, &alice_alias)
        .await
        .expect("resolve alias")
        .expect("known alias");
    assert_eq!(
        snapshot_access(&database, &alias_actor, note.note_id()).await,
        Ok(Some(NoteAccess::Manage))
    );

    assert!(
        database
            .replace_note_acl(
                &alias_actor,
                note.note_id(),
                &[NoteAclEntry::new(
                    alias_actor.principal().clone(),
                    NotePermission::Read,
                )],
                Revision::INITIAL,
                UnixMillis::new(2),
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn single_source_updates_and_purges_notes_transactionally() {
    let database = database().await;
    let note_id = note_id("0197c9bc-0000-7000-8000-000000000001");
    let note = note_seed(
        "0197c9bc-0000-7000-8000-000000000001",
        "alice",
        "First title",
    )
    .source("first body")
    .tags(&["research"])
    .build();
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
    let alice = user("alice");
    let charlie = user("charlie");
    let bob = user("bob");
    let same_subject_different_issuer = actor("https://other-id.example.test", "alice");
    let former_administrator = user("administrator");
    assert!(
        database
            .accessible_note(&alice, note_id)
            .await
            .expect("owner is visible")
            .is_some()
    );
    assert_eq!(database.accessible_note(&charlie, note_id).await, Ok(None));
    assert_eq!(
        database
            .accessible_note(&same_subject_different_issuer, note_id)
            .await,
        Ok(None)
    );
    let owner_list = database
        .list_visible_notes(&alice, &NoteListQuery::default())
        .await
        .expect("owner list");
    assert_eq!(owner_list.len(), 1);
    assert_eq!(owner_list[0].access, NoteAccess::Manage);
    assert!(
        database
            .list_visible_notes(&charlie, &NoteListQuery::default())
            .await
            .expect("non-owner list")
            .is_empty()
    );
    assert!(
        database
            .list_visible_notes(&same_subject_different_issuer, &NoteListQuery::default())
            .await
            .expect("different issuer list")
            .is_empty()
    );
    assert!(
        database
            .list_visible_notes(&former_administrator, &NoteListQuery::default())
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
                &draft("Unauthorized title", "= Denied\n\nmust not persist", &[]),
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
            &draft(
                "Updated title",
                "= Updated\n\nupdated body",
                &["research", "v3"],
            ),
            NoteLinks::default(),
            UnixMillis::new(200),
        )
        .await
        .expect("update note");
    assert_eq!(updated.revision().get(), 2);
    assert_eq!(updated.title(), "Updated title");
    assert_eq!(updated.review_status(), NoteReviewStatus::Pending);
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
    let deleted_notes = database
        .list_owned_deleted_notes(&alice)
        .await
        .expect("owner deleted list");
    assert_eq!(deleted_notes.len(), 1);
    assert_eq!(deleted_notes[0].note_id, note_id);
    assert_eq!(deleted_notes[0].title, "Updated title");
    assert_eq!(deleted_notes[0].deleted_at, UnixMillis::new(300));
    assert_eq!(
        deleted_notes[0].purge_at,
        UnixMillis::new(300 + SOFT_DELETE_RETENTION_MS)
    );
    assert_eq!(deleted_notes[0].revision, revision(3));
    assert!(
        database
            .list_owned_deleted_notes(&charlie)
            .await
            .expect("non-owner deleted list")
            .is_empty()
    );
    assert!(
        database
            .list_owned_deleted_notes(&same_subject_different_issuer)
            .await
            .expect("different issuer deleted list")
            .is_empty()
    );
    assert_eq!(database.note(note_id, false).await, Ok(None));
    let deleted = database
        .note(note_id, true)
        .await
        .expect("read deleted")
        .expect("deleted note remains");
    assert_eq!(deleted.deleted_at(), Some(UnixMillis::new(300)));
    assert_eq!(deleted.revision().get(), 3);

    let restored = database
        .restore_owned_deleted_note(&alice, note_id, revision(3), UnixMillis::new(350))
        .await
        .expect("restore note");
    assert_eq!(restored.deleted_at(), None);
    assert_eq!(restored.revision().get(), 4);
    database
        .replace_note_acl(
            &alice,
            note_id,
            &[acl_entry("bob", NotePermission::Read)],
            revision(4),
            UnixMillis::new(360),
        )
        .await
        .expect("store ACL");
    database
        .replace_math_macros(
            alice.principal(),
            &[MathMacro {
                name: "bm".into(),
                replacement: r"\boldsymbol{#1}".into(),
                argument_count: 1,
            }],
            0,
        )
        .await
        .expect("store math macro settings");
    let snapshot = database
        .export_archive_snapshot()
        .await
        .expect("export snapshot");
    let plan = RestorePlan::new(snapshot.clone(), vec![(note_id, note_id)], Vec::new())
        .expect("valid restore plan");
    let imported_database = super::empty_database().await;
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
    let nonempty_auth_database = super::database().await;
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
    let rejected_database = super::database().await;
    let empty_snapshot = rejected_database
        .export_archive_snapshot()
        .await
        .expect("empty snapshot");
    assert!(empty_snapshot.notes().is_empty());
    assert!(empty_snapshot.note_acl().is_empty());
    database
        .create_owned_item(&bibliography_item(
            "0197c9bc-0000-7000-8000-000000000099",
            &alice,
            "smith2024",
            r#"{"id":"smith2024","type":"article-journal","title":"Preserved work"}"#,
        ))
        .await
        .expect("bibliography item");
    sqlx::query("INSERT INTO note_references (source_note_id, target_note_id) VALUES (?, ?)")
        .bind(note_id.to_string())
        .bind(note_id.to_string())
        .execute(&database.pool)
        .await
        .expect("reference index");
    sqlx::query(
        "INSERT INTO note_citations (source_note_id, citation_key) VALUES (?, 'smith2024')",
    )
    .bind(note_id.to_string())
    .execute(&database.pool)
    .await
    .expect("citation index");
    database
        .soft_delete_visible_note(&alice, note_id, revision(5), UnixMillis::new(400))
        .await
        .expect("delete before purge");
    assert!(
        database
            .note_graph(&alice, &NoteGraphQuery::default())
            .await
            .expect("deleted graph")
            .notes
            .is_empty(),
        "削除中は索引を保持したまま通常の図から隠します"
    );
    assert!(
        database
            .list_owned_deleted_notes(&bob)
            .await
            .expect("shared reader deleted list")
            .is_empty(),
        "削除済みノートは共有先へ開示しません"
    );
    assert_eq!(
        database
            .restore_owned_deleted_note(&bob, note_id, revision(6), UnixMillis::new(401))
            .await,
        Err(crate::notes::RestoreNoteError::Store(
            SqliteStoreError::NotFound
        )),
        "共有先には削除済みノートの存在を開示しません"
    );
    assert_eq!(
        database
            .restore_owned_deleted_note(
                &same_subject_different_issuer,
                note_id,
                revision(6),
                UnixMillis::new(401),
            )
            .await,
        Err(crate::notes::RestoreNoteError::Store(
            SqliteStoreError::NotFound
        ))
    );
    let restored = database
        .restore_owned_deleted_note(&alice, note_id, revision(6), UnixMillis::new(401))
        .await
        .expect("restore with ACL");
    assert_eq!(restored.revision(), revision(7));
    assert_eq!(
        snapshot_access(&database, &bob, note_id).await,
        Ok(Some(NoteAccess::Read))
    );
    let graph = database
        .note_graph(&alice, &NoteGraphQuery::default())
        .await
        .expect("restored graph");
    assert_eq!(graph.references.len(), 1);
    assert_eq!(graph.citations.len(), 1);
    assert_eq!(graph.works[0].title.as_deref(), Some("Preserved work"));
    let macros = database
        .read_math_macros(alice.principal())
        .await
        .expect("restored owner macros");
    assert_eq!(macros.macros[0].name, "bm");
    database
        .soft_delete_visible_note(&alice, note_id, revision(7), UnixMillis::new(500))
        .await
        .expect("delete before expired restoration");
    assert_eq!(
        database
            .restore_owned_deleted_note(
                &alice,
                note_id,
                revision(7),
                UnixMillis::new(500 + SOFT_DELETE_RETENTION_MS + 1)
            )
            .await,
        Err(crate::notes::RestoreNoteError::Store(
            SqliteStoreError::Conflict
        )),
        "revision競合を期限切れより先に判定します"
    );
    assert_eq!(
        database
            .restore_owned_deleted_note(
                &alice,
                note_id,
                revision(8),
                UnixMillis::new(500 + SOFT_DELETE_RETENTION_MS + 1)
            )
            .await,
        Err(crate::notes::RestoreNoteError::RetentionExpired)
    );
    assert_eq!(
        database
            .purge_deleted_before(UnixMillis::new(501))
            .await
            .expect("purge"),
        1
    );
    assert_eq!(database.note(note_id, true).await, Ok(None));
}

#[tokio::test]
async fn note_access_levels_follow_one_decision_table_and_acl_failures_roll_back() {
    let database = database().await;
    let note_id = note_id("0197c9bc-0000-7000-8000-000000000011");
    let note = note_seed("0197c9bc-0000-7000-8000-000000000011", "owner", "Title")
        .source("Body")
        .build();
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create");

    let owner = user("owner");
    let reader = user("reader");
    let same_subject_other_issuer = actor("https://other-id.example.test", "reader");
    assert_eq!(
        snapshot_access(&database, &owner, note_id).await,
        Ok(Some(NoteAccess::Manage))
    );
    assert_eq!(snapshot_access(&database, &reader, note_id).await, Ok(None));

    let changed = database
        .replace_note_acl(
            &owner,
            note_id,
            &[acl_entry("reader", NotePermission::Read)],
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
                &draft("Denied", "= Denied\n", &[]),
                NoteLinks::default(),
                UnixMillis::new(120),
            )
            .await,
        Err(SqliteStoreError::NotFound)
    );

    // issuer制限は設定値を持つapplication境界で検査する。SQLiteは解決済みprincipalだけを扱う。
    let unchanged = database
        .accessible_note(&owner, note_id)
        .await
        .expect("read after rollback")
        .expect("note")
        .note;
    assert_eq!(unchanged.revision(), changed.revision());
    assert_eq!(
        snapshot_access(&database, &reader, note_id).await,
        Ok(Some(NoteAccess::Read))
    );

    let changed = database
        .replace_note_acl(
            &owner,
            note_id,
            &[acl_entry("reader", NotePermission::Edit)],
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
                &draft("Edited", "= Edited\n", &[]),
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
async fn provenance_filters_and_review_state_follow_the_current_revision() {
    let database = database().await;
    let owner = user("owner");
    let outsider = user("outsider");
    let note_id = note_id("0197c9bc-0000-7000-8000-000000000021");
    let note = Note::create(
        note_id,
        owner.principal(),
        draft("確認対象", "= 確認対象\n\n本文", &["調査"]),
        UnixMillis::new(100),
        NoteCreationSource::Rest,
    );
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create note");

    let pending = database
        .list_visible_notes(
            &owner,
            &NoteListQuery {
                created_via: Some(NoteCreationSource::Rest),
                review_status: Some(NoteReviewStatus::Pending),
            },
        )
        .await
        .expect("pending list");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].summary.created_via, NoteCreationSource::Rest);
    assert_eq!(pending[0].summary.review_status, NoteReviewStatus::Pending);
    assert!(
        database
            .list_visible_notes(
                &owner,
                &NoteListQuery {
                    created_via: Some(NoteCreationSource::Web),
                    review_status: None,
                },
            )
            .await
            .expect("source filter")
            .is_empty()
    );

    assert_eq!(
        database
            .mark_owned_note_reviewed(&outsider, note_id, Revision::INITIAL, UnixMillis::new(110),)
            .await,
        Err(SqliteStoreError::NotFound)
    );
    let reviewed = database
        .mark_owned_note_reviewed(&owner, note_id, Revision::INITIAL, UnixMillis::new(110))
        .await
        .expect("mark reviewed");
    assert_eq!(reviewed.revision(), revision(2));
    assert_eq!(reviewed.review_status(), NoteReviewStatus::Reviewed);
    assert_eq!(
        reviewed.last_review().expect("review").revision(),
        revision(2)
    );
    assert_eq!(
        database
            .mark_owned_note_reviewed(&owner, note_id, Revision::INITIAL, UnixMillis::new(120),)
            .await,
        Err(SqliteStoreError::Conflict)
    );

    let updated = database
        .update_visible_note(
            &owner,
            note_id,
            reviewed.revision(),
            &draft("更新後", "= 更新後\n\n本文", &[]),
            NoteLinks::default(),
            UnixMillis::new(120),
        )
        .await
        .expect("update note");
    assert_eq!(updated.review_status(), NoteReviewStatus::Pending);
    assert_eq!(
        updated.last_review().expect("retained review").revision(),
        revision(2)
    );
    assert!(
        database
            .list_visible_notes(
                &owner,
                &NoteListQuery {
                    created_via: None,
                    review_status: Some(NoteReviewStatus::Reviewed),
                },
            )
            .await
            .expect("reviewed filter")
            .is_empty()
    );

    let reviewed = database
        .mark_owned_note_reviewed(&owner, note_id, updated.revision(), UnixMillis::new(130))
        .await
        .expect("review updated note");
    let acl_changed = database
        .replace_note_acl(
            &owner,
            note_id,
            &[acl_entry("reader", NotePermission::Read)],
            reviewed.revision(),
            UnixMillis::new(140),
        )
        .await
        .expect("change ACL after review");
    assert_eq!(acl_changed.review_status(), NoteReviewStatus::Pending);

    let reviewed = database
        .mark_owned_note_reviewed(
            &owner,
            note_id,
            acl_changed.revision(),
            UnixMillis::new(150),
        )
        .await
        .expect("review ACL change");
    let deleted = database
        .soft_delete_visible_note(&owner, note_id, reviewed.revision(), UnixMillis::new(160))
        .await
        .expect("delete reviewed note");
    assert_eq!(deleted.review_status(), NoteReviewStatus::Pending);
    let restored = database
        .restore_owned_deleted_note(&owner, note_id, deleted.revision(), UnixMillis::new(170))
        .await
        .expect("restore reviewed note");
    assert_eq!(restored.review_status(), NoteReviewStatus::Pending);

    let reviewed = database
        .mark_owned_note_reviewed(&owner, note_id, restored.revision(), UnixMillis::new(180))
        .await
        .expect("review restored note");
    database
        .create_owned_item(&bibliography_item(
            "0197c9bc-0000-7000-8000-0000000000a1",
            &owner,
            "smith2024",
            r#"{"id":"smith2024","type":"book","title":"Example"}"#,
        ))
        .await
        .expect("change bibliography");
    database
        .replace_math_macros(
            owner.principal(),
            &[MathMacro {
                name: "bm".into(),
                replacement: r"\boldsymbol{#1}".into(),
                argument_count: 1,
            }],
            0,
        )
        .await
        .expect("change math macros");
    let unchanged = database
        .read_owned_note_review(&owner, note_id)
        .await
        .expect("read review after external resource changes");
    assert_eq!(unchanged.revision(), reviewed.revision());
    assert_eq!(unchanged.review_status(), NoteReviewStatus::Reviewed);
}

#[tokio::test]
async fn concurrent_note_updates_accept_only_one_expected_revision() {
    let database = database().await;
    let note_id = note_id("0197c9bc-0000-7000-8000-000000000012");
    let note = note_seed("0197c9bc-0000-7000-8000-000000000012", "owner", "Title")
        .source("Body")
        .build();
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create");
    let owner = user("owner");
    let first_draft = draft("First", "= First\n", &[]);
    let second_draft = draft("Second", "= Second\n", &[]);
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
        .accessible_note(&owner, note_id)
        .await
        .expect("read after conflict")
        .expect("visible note")
        .note;
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
    let database = database().await;
    let alice = user("alice");
    let bob = user("bob");

    let shared = graph_note("0197c9bc-0000-7000-8000-000000000001", "共有するノート");
    let private = graph_note("0197c9bc-0000-7000-8000-000000000002", "共有しないノート");
    database
        .create_note(
            &shared,
            NoteLinks {
                reference_targets: &[private.note_id()],
                cited_keys: &["smith2024".to_owned()],
                attachment_ids: &[],
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
                attachment_ids: &[],
            },
        )
        .await
        .expect("create private note");
    database
        .replace_note_acl(
            &alice,
            shared.note_id(),
            &[acl_entry("bob", NotePermission::Read)],
            Revision::INITIAL,
            UnixMillis::new(200),
        )
        .await
        .expect("share the note with bob");
    database
        .create_owned_item(&bibliography_item(
            "0197c9bc-0000-7000-8000-0000000000a1",
            &alice,
            "smith2024",
            r#"{"id":"smith2024","type":"book","title":"An Example"}"#,
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
    // 引用された文献だけが出る。作成者のライブラリで解決できた題名を添える。
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

#[tokio::test]
async fn graph_rejects_semantically_corrupt_bibliography_items() {
    let database = database().await;
    let alice = user("alice");
    let note = graph_note("0197c9bc-0000-7000-8000-0000000000b1", "引用を含むノート");
    database
        .create_note(
            &note,
            NoteLinks {
                reference_targets: &[],
                cited_keys: &["smith2024".to_owned()],
                attachment_ids: &[],
            },
        )
        .await
        .expect("create note");
    let item = bibliography_item(
        "0197c9bc-0000-7000-8000-0000000000b2",
        &alice,
        "smith2024",
        r#"{"id":"smith2024","type":"book","title":"Example"}"#,
    );
    database
        .create_owned_item(&item)
        .await
        .expect("create bibliography item");
    sqlx::query("UPDATE bibliography_items SET csl_json = ? WHERE item_id = ?")
        .bind(r#"{"id":"different","type":"book","title":"Corrupt"}"#)
        .bind(item.item_id().to_string())
        .execute(&database.pool)
        .await
        .expect("inject corrupt CSL-JSON");

    assert_eq!(
        database
            .note_graph(&alice, &NoteGraphQuery::default())
            .await,
        Err(SqliteStoreError::CorruptData)
    );
}

/// 想定規模（REQ-OPS-003の約1,000ノート）で、図の問い合わせが一度で全体を返すことを確かめる。
///
/// 点と線の本数を数えるだけの試験である。所要時間は環境で変わるため上限を判定しない。
#[tokio::test]
async fn the_graph_answers_at_the_assumed_scale() {
    const NOTES: usize = 1_000;
    const WORKS: usize = 50;

    let database = database().await;
    let alice = user("alice");
    let identifiers: Vec<NoteId> = (0..NOTES)
        .map(|index| note_id(&format!("0197c9bc-0000-7000-8000-{index:012x}")))
        .collect();

    for (index, current_id) in identifiers.iter().enumerate() {
        // 鎖状につなぎ、参照の線が確実に1本ずつ増えるようにする。
        let targets = if index + 1 < NOTES {
            vec![identifiers[index + 1]]
        } else {
            Vec::new()
        };
        let cited = vec![format!("work{:04}", index % WORKS)];
        let title = format!("規模の確認 {index}");
        let note = note_seed(
            &format!("0197c9bc-0000-7000-8000-{index:012x}"),
            "alice",
            &title,
        )
        .source(format!(
            "= {title}\n\n本文と cite:work{:04}[]",
            index % WORKS
        ))
        // 半数へタグを付け、語での絞り込みがタグにも効くことを見る。
        .tags(if index % 2 == 0 { &["調査"] } else { &[] })
        .build();
        assert_eq!(note.note_id(), *current_id);
        database
            .create_note(
                &note,
                NoteLinks {
                    reference_targets: &targets,
                    cited_keys: &cited,
                    attachment_ids: &[],
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

#[tokio::test]
async fn graph_search_treats_like_metacharacters_as_text() {
    let database = database().await;
    let alice = user("alice");
    for note in [
        graph_note("0197c9bc-0000-7000-8000-0000000000d1", "進捗 100%_確認"),
        graph_note("0197c9bc-0000-7000-8000-0000000000d2", "通常の進捗"),
    ] {
        database
            .create_note(
                &note,
                NoteLinks {
                    reference_targets: &[],
                    cited_keys: &[],
                    attachment_ids: &[],
                },
            )
            .await
            .expect("create note");
    }

    for query in ["%", "_"] {
        let graph = database
            .note_graph(
                &alice,
                &NoteGraphQuery {
                    text: Some(query.into()),
                    ..NoteGraphQuery::default()
                },
            )
            .await
            .expect("literal metacharacter search");
        assert_eq!(graph.notes.len(), 1);
        assert_eq!(graph.notes[0].title, "進捗 100%_確認");
    }
}
