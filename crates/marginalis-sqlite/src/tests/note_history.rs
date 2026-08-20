use marginalis_application::NoteLinks;
use marginalis_domain::{NoteAccess, NotePermission, NoteRevisionKind, UnixMillis};

use super::*;

#[tokio::test]
async fn history_follows_current_access_and_is_purged_with_the_note() {
    let database = database().await;
    let alice = user("alice");
    let bob = user("bob");
    let note = note_seed("0197c9bc-0000-7000-8000-000000000081", "alice", "履歴")
        .source("= 履歴\n\n最初")
        .build();
    let note_id = note.note_id();
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create note");
    database
        .replace_note_acl(
            &alice,
            note_id,
            &[acl_entry("bob", NotePermission::Edit)],
            revision(1),
            UnixMillis::new(200),
        )
        .await
        .expect("share note");
    database
        .update_visible_note(
            &bob,
            note_id,
            revision(2),
            &draft("履歴", "= 履歴\n\n二番目", &[]),
            NoteLinks::default(),
            UnixMillis::new(300),
        )
        .await
        .expect("editor update");
    database
        .mark_owned_note_reviewed(&alice, note_id, revision(3), UnixMillis::new(350))
        .await
        .expect("review note");

    let summaries = database
        .list_note_revisions(&bob, note_id)
        .await
        .expect("history query")
        .expect("visible history");
    assert_eq!(
        summaries
            .iter()
            .map(|entry| (entry.revision.get(), entry.kind))
            .collect::<Vec<_>>(),
        vec![
            (4, NoteRevisionKind::Reviewed),
            (3, NoteRevisionKind::ContentUpdated),
            (2, NoteRevisionKind::AclUpdated),
            (1, NoteRevisionKind::Created),
        ]
    );
    assert_eq!(
        summaries[1].changed_by.id(),
        bob.principal_id(),
        "共有編集者が変更者として残ります"
    );
    let first = database
        .note_revision(&bob, note_id, revision(1))
        .await
        .expect("revision query")
        .expect("visible revision");
    assert_eq!(first.access, NoteAccess::Edit);
    assert_eq!(first.revision.note().source(), "= 履歴\n\n最初");

    database
        .soft_delete_visible_note(&alice, note_id, revision(4), UnixMillis::new(400))
        .await
        .expect("delete note");
    assert_eq!(
        database
            .list_note_revisions(&bob, note_id)
            .await
            .expect("former editor query"),
        None,
        "削除中の履歴は所有者以外へ見せません"
    );
    let deleted = database
        .note_revision(&alice, note_id, revision(5))
        .await
        .expect("owner history query")
        .expect("deleted revision");
    assert_eq!(deleted.access, NoteAccess::Manage);
    assert_eq!(deleted.revision.kind(), NoteRevisionKind::Deleted);

    database
        .restore_owned_deleted_note(&alice, note_id, revision(5), UnixMillis::new(450))
        .await
        .expect("restore deleted note");
    database
        .restore_visible_note_revision(
            &bob,
            note_id,
            revision(6),
            &draft("履歴", "= 履歴\n\n最初", &[]),
            NoteLinks::default(),
            UnixMillis::new(500),
        )
        .await
        .expect("restore historical source");
    let restored = database
        .note_revision(&alice, note_id, revision(7))
        .await
        .expect("restored history query")
        .expect("restored history");
    assert_eq!(restored.revision.kind(), NoteRevisionKind::HistoryRestored);
    assert_eq!(restored.revision.changed_by().id(), bob.principal_id());

    database
        .soft_delete_visible_note(&alice, note_id, revision(7), UnixMillis::new(600))
        .await
        .expect("delete before purge");
    assert_eq!(
        database
            .purge_deleted_before(UnixMillis::new(601))
            .await
            .expect("purge note"),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM note_revisions WHERE note_id = ?")
            .bind(note_id.to_string())
            .fetch_one(&database.pool)
            .await
            .expect("history count"),
        0
    );
}
