//! SQLite正本のAsciiDoc検証、可搬化、安全なHTML描画を担うadapter。

use core::fmt;

use marginalis_application::{
    NoteContent, NoteContentError, NoteProfile, NoteReferenceQuery, NoteReferenceResolution,
    NoteValidationDiagnostic, ValidatedNoteDraft,
};
use marginalis_domain::{Note, NoteDraft};

mod analysis;
mod archive;
mod configuration;
mod policy;
mod rendering;

pub use archive::{
    ARCHIVE_FORMAT, Archive, ArchiveMigrationError, ArchiveValidationError, create_archive,
    migrate_previous_archive, validate_archive,
};

pub const ADOCWEAVE_SOURCE_REVISION: &str = "b4b01c4545c03deb0fbf97c3d6a7e12ada675995";
pub(crate) const DEFAULT_SOURCE_LANGUAGES: &[&str] = &[
    "rust",
    "typescript",
    "javascript",
    "json",
    "yaml",
    "toml",
    "bash",
    "sql",
    "text",
];
pub const PINNED_ADOCWEAVE_PACKAGE_VERSION: &str = "0.19.0";
/// MCPとOpenAPIで公開する、入力規則と執筆支援情報の版。
pub const AUTHORING_PROFILE_VERSION: u32 = 5;
/// archive内のノートを受理できる入力規則の版。
pub const ARCHIVE_NOTE_PROFILE_VERSION: u32 = 4;
pub(crate) const MAX_TITLE_CHARACTERS: usize = 200;
pub(crate) const MAX_NOTE_SOURCE_BYTES: usize = 512 * 1024;
pub(crate) const MAX_TAGS: usize = 50;
pub(crate) const MAX_TAG_CHARACTERS: usize = 64;

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

    fn has_anchor(&self, source: &str, anchor: &str) -> Result<bool, NoteContentError> {
        analysis::has_anchor(source, anchor).map_err(|_| NoteContentError)
    }

    fn render(
        &self,
        note: &Note,
        resolutions: &[NoteReferenceResolution],
    ) -> Result<String, NoteContentError> {
        rendering::render_note(note, resolutions).map_err(|_| NoteContentError)
    }

    fn export(&self, note: &Note) -> Result<String, NoteContentError> {
        Ok(note.source().to_owned())
    }

    fn profile(&self) -> NoteProfile {
        policy::note_profile()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackageVersionMismatch {
    expected: &'static str,
    actual: &'static str,
}

impl fmt::Display for PackageVersionMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "AdocWeave package version mismatch: expected {}, got {}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for PackageVersionMismatch {}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderError;

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("canonical note cannot be rendered safely")
    }
}

impl std::error::Error for RenderError {}

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
        assert_ne!(AUTHORING_PROFILE_VERSION, ARCHIVE_NOTE_PROFILE_VERSION);
    }
}
