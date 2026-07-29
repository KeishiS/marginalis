use adocweave::output::diagnostics::Severity;
use adocweave::output::html::render_with_inputs;
use adocweave::resolution::{
    RenderInputs, ResolutionFailureKind, ResolutionNotice, ResolutionNoticeKind, ResolvedReference,
    ResolverFailure,
};
use marginalis_application::NoteReferenceResolution;
use marginalis_domain::Note;

use crate::RenderError;
use crate::analysis::analyze_valid_source;
use crate::configuration::{html_is_within_output_limits, output_limits, render_policy};

pub(crate) fn render_note(
    note: &Note,
    resolutions: &[NoteReferenceResolution],
) -> Result<String, RenderError> {
    let analysis = analyze_valid_source(note.source())?;
    let queries = analysis.reference_queries();
    let references = resolutions
        .iter()
        .map(|resolution| {
            let reference_index = match resolution {
                NoteReferenceResolution::Visible {
                    reference_index, ..
                }
                | NoteReferenceResolution::Hidden { reference_index } => *reference_index,
            };
            let query = queries.get(reference_index).ok_or(RenderError)?;
            Ok(match resolution {
                NoteReferenceResolution::Visible {
                    href,
                    title,
                    missing_anchor,
                    ..
                } => {
                    let resolved = ResolvedReference::resolved(query.source_range, href)
                        .with_display_text(title);
                    if *missing_anchor {
                        resolved.with_notices(vec![ResolutionNotice {
                            kind: ResolutionNoticeKind::Fallback,
                        }])
                    } else {
                        resolved
                    }
                }
                NoteReferenceResolution::Hidden { .. } => ResolvedReference::failed(
                    query.source_range,
                    ResolverFailure {
                        kind: ResolutionFailureKind::MissingTarget,
                    },
                ),
            })
        })
        .collect::<Result<Vec<_>, RenderError>>()?;
    let inputs = RenderInputs::new(references, Vec::new());
    let output = render_with_inputs(analysis.document(), &render_policy(), &inputs);
    if output
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
        || !html_is_within_output_limits(&output.html, &output_limits())
    {
        return Err(RenderError);
    }
    Ok(output.html)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_domain::{EntityId, Identity, Note, NoteDraft, NoteId, Revision, UnixMillis};

    use super::*;

    fn note(body: &str) -> Note {
        Note::restore(
            NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("UUIDv7"),
            ),
            Identity::new("https://id.example.test".into(), "alice".into()).expect("owner"),
            "A title".into(),
            format!("= A title\n\n{body}"),
            Vec::new(),
            UnixMillis::new(0),
            UnixMillis::new(1),
            Revision::INITIAL,
            None,
        )
        .expect("note")
    }

    #[test]
    fn supported_blocks_render_without_raw_markup() {
        let html = render_note(
            &note("[[local]]\nA *safe* paragraph. See <<local>>.\n\n[source,rust]\n----\nfn main() {}\n----"),
            &[],
        )
        .expect("render");
        assert!(html.contains("<strong>safe</strong>"));
        assert!(html.contains("language-rust"));
        assert!(html.contains("href=\"#local\""));
    }

    #[test]
    fn source_and_math_html_use_the_public_adocweave_contract() {
        let html = render_note(
            &note(
                ".Example <source>\n[source,rust,linenums,start=7]\n----\nfn main() {}\n----\n\nInline latexmath:[x < y].\n\n[latexmath]\n++++\nx^2 < y\n++++",
            ),
            &[],
        )
        .expect("render");

        assert!(html.contains("<figure class=\"source-block\">"));
        assert!(html.contains("<figcaption>Example &lt;source&gt;</figcaption>"));
        assert!(html.contains(
            "<pre data-language=\"rust\" data-line-numbers=\"true\" data-line-start=\"7\"><code class=\"language-rust\">fn main() {}"
        ));
        assert!(html.contains(
            "<code class=\"math-latex\" data-math-language=\"latexmath\" data-math-display=\"inline\">x &lt; y</code>"
        ));
        assert!(html.contains(
            "<pre class=\"math-latex\" data-math-language=\"latexmath\" data-math-display=\"block\"><code>x^2 &lt; y"
        ));
        assert!(!html.contains("<source>"));
        assert!(!html.contains("x < y"));
    }

    #[test]
    fn published_bibliography_example_validates_and_renders_bidirectional_links() {
        let profile = crate::policy::note_profile();
        assert!(profile.syntax.common_blocks.contains(&"bibliography"));
        assert!(
            profile
                .syntax
                .common_inlines
                .contains(&"bibliography_anchor")
        );
        assert!(
            profile
                .syntax
                .common_inlines
                .contains(&"bibliography_reference")
        );
        assert!(
            profile
                .authoring_guidance
                .iter()
                .any(|guidance| guidance.contains("Never invent or infer"))
        );
        let example = profile
            .examples
            .iter()
            .find(|example| example.kind == "bibliography")
            .expect("published bibliography example");
        assert!(example.body.contains("[bibliography]"));
        assert!(example.body.contains("[[[smith2024]]]"));
        assert!(example.body.contains("<<smith2024>>"));
        let draft = crate::validate_note_draft(NoteDraft {
            source: example.body.into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect("the published example must be accepted by create_note");
        assert_eq!(draft.title, "先行研究の整理");
        assert_eq!(draft.tags, ["文献", "研究"]);

        let rendered_note = Note::restore(
            NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000003").expect("UUIDv7"),
            ),
            Identity::new("https://id.example.test".into(), "alice".into()).expect("owner"),
            draft.title,
            draft.source,
            draft.tags,
            UnixMillis::new(0),
            UnixMillis::new(1),
            Revision::INITIAL,
            None,
        )
        .expect("validated note");
        let html = render_note(&rendered_note, &[]).expect("render bibliography example");

        assert!(html.contains("href=\"#smith2024\""));
        assert!(html.contains("id=\"smith2024\" class=\"bibliography-anchor\""));
        assert!(html.contains("id=\"_bibliography_ref_"));
        assert!(html.contains("class=\"bibliography-backref\""));
        assert!(html.contains("href=\"#_bibliography_ref_"));
    }

    #[test]
    fn resolved_and_hidden_note_references_preserve_acl_decisions() {
        let target = "0197c9bc-0000-7000-8000-000000000002";
        let source = note(&format!("xref:note:{target}[]"));
        let visible = render_note(
            &source,
            &[NoteReferenceResolution::Visible {
                reference_index: 0,
                href: format!("/notes/{target}"),
                title: "参照先".into(),
                missing_anchor: false,
            }],
        )
        .expect("visible");
        assert!(visible.contains(">参照先</a>"));

        let hidden = render_note(
            &source,
            &[NoteReferenceResolution::Hidden { reference_index: 0 }],
        )
        .expect("hidden");
        assert!(!hidden.contains("href="));
        assert!(!hidden.contains(target));
    }

    #[test]
    fn invalid_source_is_rejected_before_rendering() {
        assert_eq!(
            render_note(&note("include::secret[]"), &[]),
            Err(RenderError)
        );
    }
}
