//! SQLite正本のAsciiDoc検証、可搬化、安全なHTML描画を担うadapter。

use marginalis_application::{
    NoteCitationQuery, NoteContent, NoteContentError, NoteProfile, NoteReferenceQuery,
    NoteRenderInputs, NoteValidationDiagnostic, ValidatedNoteDraft,
};
use marginalis_domain::{Note, NoteDraft};

mod analysis;
mod configuration;
mod policy;
mod rendering;

pub const ADOCWEAVE_SOURCE_REVISION: &str = "d92447fb29dbc5fa06ea210787606b503245d073";
pub const PINNED_ADOCWEAVE_PACKAGE_VERSION: &str = "0.23.0";
/// MCPとOpenAPIで公開する、入力規則と執筆支援情報の版。
pub const AUTHORING_PROFILE_VERSION: u32 = 9;

#[derive(Clone, Copy, Debug, Default)]
pub struct AsciiDocNoteContent;

impl NoteContent for AsciiDocNoteContent {
    fn validate_draft(
        &self,
        draft: NoteDraft,
    ) -> Result<ValidatedNoteDraft, Vec<NoteValidationDiagnostic>> {
        validate_note_draft(draft)
    }

    fn reference_queries(&self, source: &str) -> Result<Vec<NoteReferenceQuery>, NoteContentError> {
        note_reference_queries(source).map_err(|_| NoteContentError)
    }

    fn citation_queries(&self, source: &str) -> Result<Vec<NoteCitationQuery>, NoteContentError> {
        analysis::citation_queries(source).map_err(|_| NoteContentError)
    }

    fn has_anchor(&self, source: &str, anchor: &str) -> Result<bool, NoteContentError> {
        analysis::has_anchor(source, anchor).map_err(|_| NoteContentError)
    }

    fn render(
        &self,
        note: &Note,
        inputs: NoteRenderInputs<'_>,
    ) -> Result<String, NoteContentError> {
        rendering::render_note(note, inputs).map_err(|_| NoteContentError)
    }

    fn export(&self, note: &Note) -> Result<String, NoteContentError> {
        Ok(note.source().to_owned())
    }

    fn profile(&self) -> NoteProfile {
        policy::note_profile()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("AdocWeave package version mismatch: expected {expected}, got {actual}")]
pub struct PackageVersionMismatch {
    expected: &'static str,
    actual: &'static str,
}

pub fn verify_runtime_package_version() -> Result<(), PackageVersionMismatch> {
    let actual = adocweave::VERSION;
    if actual == PINNED_ADOCWEAVE_PACKAGE_VERSION {
        Ok(())
    } else {
        Err(PackageVersionMismatch {
            expected: PINNED_ADOCWEAVE_PACKAGE_VERSION,
            actual,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("canonical note cannot be rendered safely")]
pub struct RenderError;

pub(crate) fn validate_note_draft(
    draft: NoteDraft,
) -> Result<ValidatedNoteDraft, Vec<NoteValidationDiagnostic>> {
    analysis::validate_draft(draft)
}

pub fn note_reference_queries(source: &str) -> Result<Vec<NoteReferenceQuery>, RenderError> {
    analysis::reference_queries(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_version_matches_the_pinned_specification() {
        assert_eq!(ADOCWEAVE_SOURCE_REVISION.len(), 40);
        verify_runtime_package_version().expect("pinned version");
    }

    #[test]
    fn authoring_profile_has_its_own_public_version() {
        assert_eq!(
            AsciiDocNoteContent.profile().profile_version,
            AUTHORING_PROFILE_VERSION
        );
    }
}
