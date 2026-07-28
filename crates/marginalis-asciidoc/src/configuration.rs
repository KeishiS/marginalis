//! Marginalis note profileで使うAdocWeave設定の単一正本。

use adocweave::output::html::{
    MathLanguagePolicy, RenderPolicy, ResourceCapabilities, SourceLanguagePolicy,
    UnknownSourceLanguage, UnresolvedReferencePresentation,
};
use adocweave::resolution::{ActiveUrlPolicy, AuthoredUrlPolicy};
use adocweave::semantic::MathLanguage;
use adocweave::{
    AnalysisLimits, AnalysisOptions, DiagnosticProfile, OutputLimits, SyntaxMode, SyntaxOptions,
};

use crate::{DEFAULT_SOURCE_LANGUAGES, MAX_NOTE_SOURCE_BYTES};

pub(crate) fn authored_url_policy() -> AuthoredUrlPolicy {
    AuthoredUrlPolicy {
        allowed_schemes: ["http".to_owned(), "https".to_owned()].into(),
        allow_relative: false,
    }
}

pub(crate) fn analysis_options() -> AnalysisOptions {
    let mut diagnostics = DiagnosticProfile::default();
    diagnostics.lint.authored_url_policy = authored_url_policy();
    AnalysisOptions {
        syntax: SyntaxOptions {
            syntax_mode: SyntaxMode::Strict,
            limits: AnalysisLimits {
                max_input_bytes: MAX_NOTE_SOURCE_BYTES as u32,
                ..AnalysisLimits::default()
            },
        },
        diagnostics,
    }
}

pub(crate) fn render_policy() -> RenderPolicy {
    RenderPolicy {
        active_urls: ActiveUrlPolicy {
            allowed_schemes: ["http".to_owned(), "https".to_owned()].into(),
            allow_authored_relative: false,
            allow_resolved_relative: false,
            allow_resolved_root_relative: true,
            allow_data_uris: false,
        },
        source_languages: SourceLanguagePolicy {
            allowed: Some(
                DEFAULT_SOURCE_LANGUAGES
                    .iter()
                    .map(|language| (*language).to_owned())
                    .collect(),
            ),
            unknown: UnknownSourceLanguage::Diagnostic,
        },
        math_languages: MathLanguagePolicy {
            allowed: [MathLanguage::Latex].into(),
        },
        resources: ResourceCapabilities {
            images: false,
            media: false,
        },
        unresolved_references: UnresolvedReferencePresentation::LabelOnly,
        ..RenderPolicy::default()
    }
}

pub(crate) const fn output_limits() -> OutputLimits {
    OutputLimits {
        max_output_bytes: 50 * 1024 * 1024,
    }
}

pub(crate) fn html_is_within_output_limits(html: &str, limits: &OutputLimits) -> bool {
    u32::try_from(html.len()).is_ok_and(|length| length <= limits.max_output_bytes)
}
