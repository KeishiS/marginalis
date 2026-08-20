use marginalis_application::NoteLinks;

use super::*;

fn attachment(id: &str, note_id: NoteId, actor: &Actor) -> marginalis_domain::StoredAttachment {
    AttachmentDraft::new(
        "result.png".into(),
        b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01payload".to_vec(),
    )
    .expect("valid image")
    .into_stored(
        id.parse::<AttachmentId>().expect("attachment ID"),
        note_id,
        UnixMillis::new(150),
        actor.principal().clone(),
    )
}

#[tokio::test]
async fn attachment_access_and_revision_references_are_enforced_in_sqlite() {
    let database = database().await;
    let alice = user("alice");
    let bob = user("bob");
    let note = note_seed(
        "0197c9bc-0000-7000-8000-000000000001",
        "alice",
        "attachment",
    )
    .build();
    database
        .create_note(&note, NoteLinks::default())
        .await
        .expect("create note");

    let unused = attachment(
        "0197c9bc-0000-7000-8000-0000000000a1",
        note.note_id(),
        &alice,
    );
    database
        .create_note_attachment(&alice, &unused)
        .await
        .expect("store unused attachment");
    assert_eq!(
        database.list_note_attachments(&bob, note.note_id()).await,
        Ok(None)
    );
    assert_eq!(
        database
            .note_attachment(&bob, note.note_id(), unused.metadata().attachment_id(),)
            .await,
        Ok(None)
    );
    database
        .delete_unused_note_attachment(&alice, note.note_id(), unused.metadata().attachment_id())
        .await
        .expect("delete an unreferenced attachment");

    let used = attachment(
        "0197c9bc-0000-7000-8000-0000000000a2",
        note.note_id(),
        &alice,
    );
    let used_id = used.metadata().attachment_id();
    database
        .create_note_attachment(&alice, &used)
        .await
        .expect("store attachment");
    database
        .update_visible_note(
            &alice,
            note.note_id(),
            Revision::INITIAL,
            &draft(
                "attachment",
                &format!("= attachment\n\nimage::attachment:{used_id}[]"),
                &[],
            ),
            NoteLinks {
                attachment_ids: &[used_id],
                ..NoteLinks::default()
            },
            UnixMillis::new(200),
        )
        .await
        .expect("reference attachment from a new revision");

    assert_eq!(
        database
            .delete_unused_note_attachment(&alice, note.note_id(), used_id)
            .await,
        Err(SqliteStoreError::Conflict)
    );
    let listed = database
        .list_note_attachments(&alice, note.note_id())
        .await
        .expect("list attachments")
        .expect("visible note");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].attachment_id(), used_id);
    assert_eq!(
        database
            .note_attachment(&alice, note.note_id(), used_id)
            .await
            .expect("read attachment")
            .expect("stored attachment")
            .bytes(),
        used.bytes()
    );
}
