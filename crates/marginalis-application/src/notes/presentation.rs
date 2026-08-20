//! ノートのプレビュー、描画、書き出し、グラフビュー。

use std::collections::HashMap;

use marginalis_domain::{
    Actor, Identity, Note, NoteCreationSource, NoteDraft, NoteId, NoteSummary,
};

use crate::{NotePreview, NoteProfile, NoteRenderContext, NoteUseCaseError, ValidatedNoteDraft};

use super::{
    NoteApplication, NoteGraph, NoteGraphQuery, NoteReferenceQuery, NoteReferenceResolution,
    NoteRenderInputs, map_owner_resource_error, reference_targets,
};

impl NoteApplication {
    async fn render_preview(
        &self,
        actor: &Actor,
        note_id: NoteId,
        owner: &Identity,
        validated: ValidatedNoteDraft,
        context: &NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        let ValidatedNoteDraft {
            draft,
            mut diagnostics,
            reference_queries,
            citation_queries,
            citation_style,
            source_spans,
        } = validated;
        let note = Note::create(
            note_id,
            owner,
            draft,
            self.clock.now(),
            NoteCreationSource::Unknown,
        );
        let target_ids = reference_targets(&reference_queries);
        let targets = self
            .queries
            .visible_notes_by_id(actor, &target_ids)
            .await
            .map_err(NoteUseCaseError::from)?;
        let resolutions = self.reference_resolutions(&targets, context, &reference_queries)?;
        let citations = self
            .citation_resolutions(owner, &citation_queries, citation_style)
            .await?;
        let html = self
            .content
            .render(
                &note,
                NoteRenderInputs {
                    references: &resolutions,
                    citations: &citations.resolutions,
                    bibliography: &citations.entries,
                },
            )
            .map_err(|_| NoteUseCaseError::RenderFailed)?;
        diagnostics.extend(citations.diagnostics);
        let math_macros = self
            .math_macros
            .read_math_macros(owner)
            .await
            .map_err(map_owner_resource_error)?
            .macros;
        Ok(NotePreview {
            html,
            diagnostics,
            math_macros,
            spans: source_spans,
        })
    }

    fn reference_resolutions(
        &self,
        targets: &[Note],
        context: &NoteRenderContext,
        queries: &[NoteReferenceQuery],
    ) -> Result<Vec<NoteReferenceResolution>, NoteUseCaseError> {
        let targets = targets
            .iter()
            .cloned()
            .map(|note| (note.note_id(), note))
            .collect::<HashMap<_, _>>();
        let mut resolutions = Vec::with_capacity(queries.len());
        for query in queries {
            let Some(target) = targets.get(&query.target_note_id) else {
                resolutions.push(NoteReferenceResolution::Hidden {
                    reference_index: query.reference_index,
                });
                continue;
            };
            let missing_anchor = match query.anchor.as_deref() {
                Some(anchor) => !self
                    .content
                    .has_anchor(target.source(), anchor)
                    .map_err(|_| NoteUseCaseError::Unavailable)?,
                None => false,
            };
            let href = self
                .links
                .href(
                    context,
                    target.note_id(),
                    (!missing_anchor)
                        .then_some(query.anchor.as_deref())
                        .flatten(),
                )
                .ok_or(NoteUseCaseError::Unavailable)?;
            resolutions.push(NoteReferenceResolution::Visible {
                reference_index: query.reference_index,
                href,
                title: target.title().to_owned(),
                missing_anchor,
            });
        }
        Ok(resolutions)
    }
}

impl NoteApplication {
    pub async fn preview_new_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        let validated = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        self.render_preview(
            &actor,
            NoteId::new(self.random.uuid_v7()),
            actor.identity(),
            validated,
            &context,
        )
        .await
    }

    pub async fn preview_note_update(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        let accessible = self
            .queries
            .accessible_note(&actor, note_id)
            .await
            .map_err(NoteUseCaseError::from)?
            .filter(|accessible| {
                accessible
                    .access
                    .allows(marginalis_domain::NoteAccess::Edit)
            })
            .ok_or(NoteUseCaseError::NotFound)?;
        let validated = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        self.render_preview(
            &actor,
            accessible.note.note_id(),
            accessible.note.owner(),
            validated,
            &context,
        )
        .await
    }

    pub fn export_note_source(&self, note: &Note) -> Result<String, NoteUseCaseError> {
        self.content
            .export(note)
            .map_err(|_| NoteUseCaseError::Unavailable)
    }

    pub async fn read_note_graph(
        &self,
        actor: Actor,
        query: NoteGraphQuery,
    ) -> Result<NoteGraph, NoteUseCaseError> {
        let graph = self
            .queries
            .note_graph(&actor, &query)
            .await
            .map_err(NoteUseCaseError::from)?;
        // 起点からの絞り込みは、閲覧できる範囲が確定した後で行う。認可の判断はrepositoryが
        // 済ませており、ここで扱うのは表示範囲だけである。
        Ok(match query.origin {
            Some(origin) => graph.within(origin, query.depth.unwrap_or(1)),
            None => graph,
        })
    }

    pub fn note_profile(&self) -> NoteProfile {
        let mut profile = self.content.profile();
        if !profile
            .advisory_rules
            .iter()
            .any(|rule| rule.code == super::citations::UNKNOWN_CITATION_KEY_CODE)
        {
            profile.advisory_rules.push(crate::NoteProfileAdvisoryRule {
                code: super::citations::UNKNOWN_CITATION_KEY_CODE,
                description: super::citations::UNKNOWN_CITATION_KEY_DESCRIPTION,
                severity: crate::NoteAdvisorySeverity::Warning,
            });
        }
        profile
    }

    pub async fn read_note_view(
        &self,
        actor: Actor,
        note_id: NoteId,
        context: NoteRenderContext,
    ) -> Result<crate::NoteView, NoteUseCaseError> {
        let mut snapshot = self
            .queries
            .note_view_snapshot(&actor, note_id)
            .await
            .map_err(NoteUseCaseError::from)?
            .ok_or(NoteUseCaseError::NotFound)?;
        sort_related_notes(&mut snapshot.related.outgoing);
        sort_related_notes(&mut snapshot.related.incoming);
        let reference_queries = self
            .content
            .reference_queries(snapshot.note.source())
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        let resolutions =
            self.reference_resolutions(&snapshot.reference_targets, &context, &reference_queries)?;
        let citation_queries = self
            .content
            .citation_queries(snapshot.note.source())
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        let citation_style = self
            .content
            .citation_style(snapshot.note.source())
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        let citations = self
            .citation_resolutions(snapshot.note.owner(), &citation_queries, citation_style)
            .await?;
        let html = self
            .content
            .render(
                &snapshot.note,
                NoteRenderInputs {
                    references: &resolutions,
                    citations: &citations.resolutions,
                    bibliography: &citations.entries,
                },
            )
            .map_err(|_| NoteUseCaseError::RenderFailed)?;
        let math_macros = self
            .math_macros
            .read_math_macros(snapshot.note.owner())
            .await
            .map_err(map_owner_resource_error)?
            .macros;
        Ok(crate::NoteView {
            note: snapshot.note,
            access: snapshot.access,
            html,
            related: snapshot.related,
            math_macros,
        })
    }
}

fn sort_related_notes(notes: &mut [NoteSummary]) {
    notes.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.note_id.to_string().cmp(&right.note_id.to_string()))
    });
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::Ordering};

    use marginalis_domain::{
        Actor, EntityId, Note, NoteAccess, NoteCreationSource, NoteDraft, NoteId, NoteRestore,
        NoteReviewTracking, Revision, UnixMillis,
    };

    use crate::NoteAdvisorySeverity;

    use super::*;
    use crate::notes::test_support::{
        AcceptContent, CitingContent, EmptyLibrary, MemoryNotes, NoMathMacros, OneItemLibrary,
        OwnerMathMacros, note_application,
    };

    #[test]
    fn profile_includes_the_application_citation_warning() {
        let repository = Arc::new(MemoryNotes::default());
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );

        assert!(
            application
                .note_profile()
                .advisory_rules
                .iter()
                .any(|rule| {
                    rule.code == super::super::citations::UNKNOWN_CITATION_KEY_CODE
                        && rule.description
                            == super::super::citations::UNKNOWN_CITATION_KEY_DESCRIPTION
                        && rule.severity == NoteAdvisorySeverity::Warning
                })
        );
    }

    #[tokio::test]
    async fn preview_preserves_advisories_without_reanalysis() {
        let repository = Arc::new(MemoryNotes::default());
        let content = Arc::new(AcceptContent::default());
        let application = note_application(
            &repository,
            content.clone(),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let actor =
            Actor::try_new("https://id.example.test".into(), "alice".into()).expect("valid actor");
        let draft = NoteDraft {
            source: "= Warning\n\nbody".into(),
            title: "Warning".into(),
            tags: Vec::new(),
        };

        let preview = application
            .preview_new_note(
                actor.clone(),
                draft.clone(),
                NoteRenderContext {
                    note_path_prefix: "/api/v3/notes".into(),
                },
            )
            .await
            .expect("warning does not reject preview");
        assert_eq!(preview.diagnostics.len(), 1);
        assert_eq!(preview.diagnostics[0].code, "test-advisory");
        assert_eq!(
            preview.diagnostics[0].severity,
            NoteAdvisorySeverity::Warning
        );
        assert_eq!(content.reference_query_calls.load(Ordering::Relaxed), 0);

        application
            .create_note(
                actor,
                draft,
                crate::NoteWritePolicy::AllowAdvisories,
                NoteCreationSource::Web,
            )
            .await
            .expect("warning does not reject save");
        assert_eq!(repository.notes.lock().expect("notes lock").len(), 1);
    }

    #[tokio::test]
    async fn update_preview_uses_owner_resources_and_requires_edit_access() {
        let repository = Arc::new(MemoryNotes::default());
        let note_id = NoteId::new(
            "0197c9bc-0000-7000-8000-000000000031"
                .parse::<EntityId>()
                .expect("UUIDv7"),
        );
        repository.notes.lock().expect("notes lock").push(
            Note::restore(NoteRestore {
                note_id,
                owner: OneItemLibrary::owner(),
                draft: NoteDraft {
                    title: "共有されたノート".into(),
                    source: "= 共有されたノート\n\n本文".into(),
                    tags: Vec::new(),
                },
                created_at: UnixMillis::new(0),
                updated_at: UnixMillis::new(1),
                revision: Revision::INITIAL,
                deleted_at: None,
                created_via: NoteCreationSource::Mcp,
                review: NoteReviewTracking::pending(),
            })
            .expect("stored note"),
        );
        let application = note_application(
            &repository,
            Arc::new(CitingContent {
                keys: vec!["smith2024".into()],
            }),
            Arc::new(OneItemLibrary),
            Arc::new(OwnerMathMacros),
        );
        let editor =
            Actor::try_new("https://id.example.test".into(), "bob".into()).expect("valid actor");
        let draft = NoteDraft {
            source: "= 共有されたノート\n\n本文 cite:[smith2024]".into(),
            title: "共有されたノート".into(),
            tags: Vec::new(),
        };

        let preview = application
            .preview_note_update(
                editor.clone(),
                note_id,
                draft.clone(),
                NoteRenderContext {
                    note_path_prefix: "/notes".into(),
                },
            )
            .await
            .expect("shared note preview");
        assert!(preview.diagnostics.is_empty());
        assert_eq!(preview.math_macros[0].name, "bm");

        *repository.accessible_as.lock().expect("access lock") = Some(NoteAccess::Read);
        assert_eq!(
            application
                .preview_note_update(
                    editor,
                    note_id,
                    draft,
                    NoteRenderContext {
                        note_path_prefix: "/notes".into(),
                    },
                )
                .await,
            Err(NoteUseCaseError::NotFound)
        );
    }
}
