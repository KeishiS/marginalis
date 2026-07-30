//! Marginalis note profileで使うAdocWeave設定の単一正本。

use adocweave::output::diagnostics::{MACRO_BOUNDARY, RuleSettings, Severity};
use adocweave::output::html::{
    MathLanguagePolicy, RenderPolicy, ResourceCapabilities, SourceLanguagePolicy,
    UnknownSourceLanguage, UnresolvedReferencePresentation,
};
use adocweave::resolution::{ActiveUrlPolicy, AuthoredUrlPolicy};
use adocweave::semantic::MathLanguage;
use adocweave::{
    AnalysisLimits, AnalysisOptions, DiagnosticProfile, OutputLimits, SyntaxMode, SyntaxOptions,
    semantic::ExternalAttributes,
};

use marginalis_domain::NOTE_POLICY;

/// [`NOTE_POLICY`]が許可するURLスキームの集合。
fn allowed_url_schemes() -> std::collections::BTreeSet<String> {
    NOTE_POLICY
        .allowed_url_schemes
        .iter()
        .map(|scheme| (*scheme).to_owned())
        .collect()
}

/// AdocWeaveが受け取る入力バイト数の上限。
///
/// [`NOTE_POLICY`]の値は`usize`だが、AdocWeaveは`u32`で受け取る。上限を超える設定を
/// 黙って切り詰めないよう、変換に失敗した場合は`u32`の最大値へ丸めず失敗させる。
fn source_byte_limit() -> u32 {
    u32::try_from(NOTE_POLICY.max_source_bytes).expect("source byte limit fits in u32")
}

/// [`NOTE_POLICY`]が許可する数式言語をAdocWeaveの表現へ対応付ける。
fn allowed_math_languages() -> std::collections::BTreeSet<MathLanguage> {
    NOTE_POLICY
        .allowed_math_languages
        .iter()
        .map(|language| match *language {
            "latexmath" => MathLanguage::Latex,
            other => panic!("未対応の数式言語がnote policyにあります: {other}"),
        })
        .collect()
}

pub(crate) fn authored_url_policy() -> AuthoredUrlPolicy {
    AuthoredUrlPolicy {
        allowed_schemes: allowed_url_schemes(),
        allow_relative: false,
    }
}

pub(crate) fn analysis_options() -> AnalysisOptions {
    let mut diagnostics = DiagnosticProfile::default();
    diagnostics.lint.authored_url_policy = authored_url_policy();
    diagnostics.lint.set_rule(
        MACRO_BOUNDARY,
        RuleSettings {
            enabled: true,
            severity: Severity::Warning,
        },
    );
    AnalysisOptions {
        syntax: SyntaxOptions {
            syntax_mode: SyntaxMode::Strict,
            limits: AnalysisLimits {
                max_input_bytes: source_byte_limit(),
                ..AnalysisLimits::default()
            },
        },
        diagnostics,
        attributes: ExternalAttributes::default(),
    }
}

pub(crate) fn render_policy() -> RenderPolicy {
    RenderPolicy {
        active_urls: ActiveUrlPolicy {
            allowed_schemes: allowed_url_schemes(),
            allow_authored_relative: false,
            allow_resolved_relative: false,
            allow_resolved_root_relative: true,
            allow_data_uris: false,
        },
        source_languages: SourceLanguagePolicy {
            allowed: Some(
                NOTE_POLICY
                    .allowed_source_languages
                    .iter()
                    .map(|language| (*language).to_owned())
                    .collect(),
            ),
            unknown: UnknownSourceLanguage::Diagnostic,
        },
        math_languages: MathLanguagePolicy {
            allowed: allowed_math_languages(),
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
        max_output_bytes: NOTE_POLICY.max_output_bytes,
    }
}

pub(crate) fn html_is_within_output_limits(html: &str, limits: &OutputLimits) -> bool {
    u32::try_from(html.len()).is_ok_and(|length| length <= limits.max_output_bytes)
}

#[cfg(test)]
mod tests {
    use adocweave::output::diagnostics::{ASCIIDOC_FILE_LINK, MACRO_BOUNDARY, NON_ASCIIDOC_XREF};

    use super::*;

    #[test]
    fn note_profile_keeps_url_rendering_and_lint_rules_together() {
        let analysis = analysis_options();
        assert!(!analysis.diagnostics.lint.authored_url_policy.allow_relative);
        assert!(analysis.diagnostics.lint.rule(ASCIIDOC_FILE_LINK).enabled);
        assert!(analysis.diagnostics.lint.rule(NON_ASCIIDOC_XREF).enabled);
        assert!(analysis.diagnostics.lint.rule(MACRO_BOUNDARY).enabled);
        assert_eq!(
            analysis.diagnostics.lint.rule(MACRO_BOUNDARY).severity,
            Severity::Warning
        );

        let rendering = render_policy();
        assert!(!rendering.active_urls.allow_authored_relative);
        assert!(!rendering.active_urls.allow_resolved_relative);
        assert!(rendering.active_urls.allow_resolved_root_relative);
        assert!(!rendering.active_urls.allow_data_uris);
        assert!(!rendering.resources.images);
        assert!(!rendering.resources.media);
        assert_eq!(
            rendering.source_languages.unknown,
            UnknownSourceLanguage::Diagnostic
        );
        assert_eq!(
            output_limits().max_output_bytes,
            NOTE_POLICY.max_output_bytes
        );
    }

    #[test]
    fn rendered_output_limit_is_inclusive() {
        let limits = OutputLimits {
            max_output_bytes: 4,
        };
        assert!(html_is_within_output_limits("1234", &limits));
        assert!(!html_is_within_output_limits("12345", &limits));
    }
}
