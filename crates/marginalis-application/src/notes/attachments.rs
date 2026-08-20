//! 添付画像のupload、認可済み取得、本文参照の解決。

use std::collections::HashMap;

use marginalis_domain::{
    Actor, AttachmentDraft, AttachmentId, AttachmentMetadata, NoteAccess, NoteId,
    NoteValidationTarget, StoredAttachment,
};

use crate::{NoteRenderContext, NoteUseCaseError, NoteValidationCode, NoteValidationDiagnostic};

use super::{NoteApplication, NoteAttachmentQuery, NoteAttachmentResolution};

impl NoteApplication {
    pub async fn upload_note_attachment(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: AttachmentDraft,
    ) -> Result<AttachmentMetadata, NoteUseCaseError> {
        let accessible = self
            .queries
            .accessible_note(&actor, note_id)
            .await
            .map_err(NoteUseCaseError::from)?
            .filter(|entry| entry.access.allows(NoteAccess::Edit))
            .ok_or(NoteUseCaseError::NotFound)?;
        if accessible.note.deleted_at().is_some() {
            return Err(NoteUseCaseError::NotFound);
        }
        let attachment = draft.into_stored(
            AttachmentId::new(self.random.uuid_v7()),
            note_id,
            self.clock.now(),
            actor.principal().clone(),
        );
        self.commands
            .create_note_attachment(&actor, &attachment)
            .await
            .map_err(NoteUseCaseError::from)?;
        Ok(attachment.metadata().clone())
    }

    pub async fn list_note_attachments(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<Vec<AttachmentMetadata>, NoteUseCaseError> {
        self.queries
            .list_note_attachments(&actor, note_id)
            .await
            .map_err(NoteUseCaseError::from)?
            .ok_or(NoteUseCaseError::NotFound)
    }

    pub async fn read_note_attachment(
        &self,
        actor: Actor,
        note_id: NoteId,
        attachment_id: AttachmentId,
    ) -> Result<StoredAttachment, NoteUseCaseError> {
        self.queries
            .note_attachment(&actor, note_id, attachment_id)
            .await
            .map_err(NoteUseCaseError::from)?
            .ok_or(NoteUseCaseError::NotFound)
    }

    pub async fn delete_unused_note_attachment(
        &self,
        actor: Actor,
        note_id: NoteId,
        attachment_id: AttachmentId,
    ) -> Result<(), NoteUseCaseError> {
        self.commands
            .delete_unused_note_attachment(&actor, note_id, attachment_id)
            .await
            .map_err(NoteUseCaseError::from)
    }

    pub(super) async fn resolve_note_attachments(
        &self,
        actor: &Actor,
        note_id: NoteId,
        queries: &[NoteAttachmentQuery],
        context: &NoteRenderContext,
    ) -> Result<Vec<NoteAttachmentResolution>, NoteUseCaseError> {
        let metadata = self
            .attachment_metadata_by_id(actor, note_id, queries)
            .await?;
        queries
            .iter()
            .map(|query| {
                let entry = metadata
                    .get(&query.attachment_id)
                    .expect("validated attachment query has metadata");
                let href = self
                    .links
                    .attachment_href(context, note_id, entry.attachment_id())
                    .ok_or(NoteUseCaseError::Unavailable)?;
                Ok(NoteAttachmentResolution {
                    attachment_index: query.attachment_index,
                    href,
                    media_type: entry.media_type(),
                    byte_length: entry.byte_length(),
                })
            })
            .collect()
    }

    pub(super) async fn validate_note_attachment_references(
        &self,
        actor: &Actor,
        note_id: NoteId,
        queries: &[NoteAttachmentQuery],
    ) -> Result<(), NoteUseCaseError> {
        self.attachment_metadata_by_id(actor, note_id, queries)
            .await
            .map(|_| ())
    }

    async fn attachment_metadata_by_id(
        &self,
        actor: &Actor,
        note_id: NoteId,
        queries: &[NoteAttachmentQuery],
    ) -> Result<HashMap<AttachmentId, AttachmentMetadata>, NoteUseCaseError> {
        if queries.is_empty() {
            return Ok(HashMap::new());
        }
        let metadata = self
            .queries
            .list_note_attachments(actor, note_id)
            .await
            .map_err(NoteUseCaseError::from)?
            .ok_or(NoteUseCaseError::NotFound)?
            .into_iter()
            .map(|entry| (entry.attachment_id(), entry))
            .collect::<HashMap<_, _>>();
        let missing = queries
            .iter()
            .filter(|query| !metadata.contains_key(&query.attachment_id))
            .map(|query| NoteValidationDiagnostic {
                code: NoteValidationCode::InvalidAttachmentReference
                    .as_str()
                    .to_owned(),
                target: NoteValidationTarget::Source,
                span: Some(query.span),
                position: Some(query.position),
                message: "attachment reference must name an image stored by this note".into(),
            })
            .collect::<Vec<_>>();
        if missing.is_empty() {
            Ok(metadata)
        } else {
            Err(NoteUseCaseError::Validation(missing))
        }
    }
}

pub(super) fn rejected_attachment_references(queries: &[NoteAttachmentQuery]) -> NoteUseCaseError {
    NoteUseCaseError::Validation(
        queries
            .iter()
            .map(|query| NoteValidationDiagnostic {
                code: NoteValidationCode::InvalidAttachmentReference
                    .as_str()
                    .to_owned(),
                target: NoteValidationTarget::Source,
                span: Some(query.span),
                position: Some(query.position),
                message: "attachments can be added after the note is created".into(),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use marginalis_domain::{
        AttachmentDraft, AttachmentId, NoteCreationSource, NoteDraft, NoteId, Utf8ByteSpan,
    };

    use crate::{NoteLinkResolver, NoteRenderContext, NoteSourcePosition, NoteWritePolicy};

    use super::super::test_support::{
        AcceptContent, EmptyLibrary, MemoryNotes, NoMathMacros, actor, note_application_with_links,
    };
    use super::*;

    struct RoutedLinks;

    impl NoteLinkResolver for RoutedLinks {
        fn href(
            &self,
            _context: &NoteRenderContext,
            _note_id: NoteId,
            _anchor: Option<&str>,
        ) -> Option<String> {
            None
        }

        fn attachment_href(
            &self,
            context: &NoteRenderContext,
            note_id: NoteId,
            attachment_id: AttachmentId,
        ) -> Option<String> {
            Some(format!(
                "{}/api/v3/notes/{note_id}/attachments/{attachment_id}/content",
                context.base_path.trim_end_matches('/')
            ))
        }
    }

    #[tokio::test]
    async fn attachment_resolution_uses_the_transport_link_port() {
        let repository = Arc::new(MemoryNotes::default());
        let application = note_application_with_links(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
            Arc::new(RoutedLinks),
        );
        let actor = actor("alice", 1);
        let note = application
            .create_note(
                actor.clone(),
                NoteDraft {
                    title: "図".into(),
                    source: "= 図".into(),
                    tags: Vec::new(),
                },
                NoteWritePolicy::AllowAdvisories,
                NoteCreationSource::Web,
            )
            .await
            .expect("create note");
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01".to_vec();
        png.extend_from_slice(b"payload");
        let attachment = application
            .upload_note_attachment(
                actor.clone(),
                note.note_id(),
                AttachmentDraft::new("figure.png".into(), png).expect("valid image"),
            )
            .await
            .expect("upload attachment");

        let resolved = application
            .resolve_note_attachments(
                &actor,
                note.note_id(),
                &[NoteAttachmentQuery {
                    attachment_index: 0,
                    attachment_id: attachment.attachment_id(),
                    span: Utf8ByteSpan { start: 0, end: 1 },
                    position: NoteSourcePosition { line: 1, column: 1 },
                }],
                &NoteRenderContext {
                    base_path: "/knowledge".into(),
                },
            )
            .await
            .expect("resolve attachment");

        assert_eq!(
            resolved[0].href,
            format!(
                "/knowledge/api/v3/notes/{}/attachments/{}/content",
                note.note_id(),
                attachment.attachment_id()
            )
        );
    }
}
