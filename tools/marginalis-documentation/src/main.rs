//! Marginalisのリポジトリ文書に固有の関係を検証する。
//!
//! AsciiDocの構文認識はAdocWeaveへ委ね、このcrateでは複数文書をまたぐ明示IDの解決など、
//! 一つの文書だけでは決められない規則だけを扱う。

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use adocweave::semantic::{Inline, ReferenceDestination, ReferenceTargetKind, SemanticNode, walk};
use adocweave::{AnalysisOptions, Engine};

fn main() {
    if let Err(error) = run(env::args().skip(1)) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let mut arguments = arguments;
    match arguments.next().as_deref() {
        Some("check-xrefs") => check_xrefs(arguments),
        _ => Err(usage()),
    }
}

fn usage() -> String {
    concat!(
        "使用方法:\n",
        "  marginalis-documentation check-xrefs --project-root ROOT DOCUMENT...",
    )
    .to_owned()
}

fn check_xrefs(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    if arguments.next().as_deref() != Some("--project-root") {
        return Err(usage());
    }
    let project_root = arguments
        .next()
        .ok_or_else(|| "project rootを指定してください。".to_owned())?;
    let document_paths = arguments.map(PathBuf::from).collect::<Vec<_>>();
    if document_paths.is_empty() {
        return Err("検査対象のAsciiDoc文書がありません。".to_owned());
    }

    let documents = document_paths
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path)
                .map_err(|error| format!("文書を読めません: {}: {error}", path.display()))?;
            Ok((path, source))
        })
        .collect::<Result<Vec<_>, String>>()?;
    validate_cross_document_xrefs(Path::new(&project_root), &documents)
}

#[derive(Debug)]
struct DocumentFacts {
    explicit_ids: BTreeSet<String>,
    references: Vec<(String, Option<String>)>,
}

fn validate_cross_document_xrefs(
    project_root: &Path,
    documents: &[(PathBuf, String)],
) -> Result<(), String> {
    let engine = Engine::new(AnalysisOptions::default());
    let mut corpus = BTreeMap::new();
    let mut errors = Vec::new();

    for (path, source) in documents {
        let relative = project_relative_path(project_root, path)?;
        let analysis = engine
            .analyze(source)
            .map_err(|error| format!("文書を解析できません: {}: {error}", path.display()))?;
        walk(analysis.document(), |node| match node {
            SemanticNode::Inline(Inline::Text(text)) => {
                for macro_name in ["xref:", "link:"] {
                    if !text.value.contains(macro_name) {
                        continue;
                    }
                    errors.push(format!(
                            "{}: {macro_name}記法が通常の文章として解釈されています。記法の前に空白を置いてください",
                            relative.display()
                        ));
                }
            }
            SemanticNode::Inline(Inline::Passthrough { .. }) => errors.push(format!(
                "{}: 人間向け文書ではpassthrough記法を使用できません",
                relative.display()
            )),
            SemanticNode::ElementAttribute(attribute)
                if attribute.name.as_deref() == Some("cols")
                    && attribute
                        .value
                        .trim_matches('"')
                        .split(',')
                        .any(|column| column.trim().is_empty()) =>
            {
                errors.push(format!(
                    "{}: 表のcols属性では各列を明示してください",
                    relative.display()
                ));
            }
            _ => {}
        });
        let mut explicit_ids = analysis
            .document()
            .anchors()
            .iter()
            .filter(|anchor| anchor.valid)
            .map(|anchor| anchor.id.clone())
            .collect::<BTreeSet<_>>();
        explicit_ids.extend(
            analysis
                .reference_targets()
                .iter()
                .filter(|target| target.kind == ReferenceTargetKind::InlineAnchor)
                .map(|target| target.id.clone()),
        );
        let references = analysis
            .references()
            .iter()
            .filter_map(|reference| match &reference.authored_destination {
                ReferenceDestination::Document {
                    document, anchor, ..
                } if Path::new(document)
                    .extension()
                    .is_some_and(|value| value == "adoc") =>
                {
                    Some((document.clone(), anchor.clone()))
                }
                _ => None,
            })
            .collect();
        if corpus
            .insert(
                relative,
                DocumentFacts {
                    explicit_ids,
                    references,
                },
            )
            .is_some()
        {
            return Err(format!(
                "検査対象の文書pathが重複しています: {}",
                path.display()
            ));
        }
    }

    for (source_path, facts) in &corpus {
        for (document, anchor) in &facts.references {
            let joined = source_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(document);
            let Some(target_path) = normalize_relative_path(&joined) else {
                errors.push(format!(
                    "{}: xrefがproject rootの外を指しています: {document}",
                    source_path.display()
                ));
                continue;
            };
            let Some(target) = corpus.get(&target_path) else {
                errors.push(format!(
                    "{}: xrefのAsciiDoc文書が検査対象にありません: {}",
                    source_path.display(),
                    target_path.display()
                ));
                continue;
            };
            let Some(anchor) = anchor.as_ref().filter(|anchor| !anchor.is_empty()) else {
                errors.push(format!(
                    "{}: 文書間xrefは明示IDを指定してください: {document}",
                    source_path.display()
                ));
                continue;
            };
            if !target.explicit_ids.contains(anchor) {
                errors.push(format!(
                    "{}: xrefの明示IDが参照先にありません: {}#{anchor}",
                    source_path.display(),
                    target_path.display()
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn project_relative_path(project_root: &Path, path: &Path) -> Result<PathBuf, String> {
    let relative = if path.is_absolute() {
        path.strip_prefix(project_root)
            .map_err(|_| format!("文書がproject rootの外にあります: {}", path.display()))?
    } else {
        path.strip_prefix(project_root).unwrap_or(path)
    };
    normalize_relative_path(relative)
        .ok_or_else(|| format!("安全でない文書pathです: {}", path.display()))
}

fn normalize_relative_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(path: &str, source: &str) -> (PathBuf, String) {
        (PathBuf::from(path), source.to_owned())
    }

    #[test]
    fn cross_document_xref_requires_an_existing_explicit_id() {
        let valid = [
            document("index.adoc", "= 入口\n\nxref:guide.adoc#details[詳細]\n"),
            document("guide.adoc", "= 案内\n\n[#details]\n== 詳細\n"),
        ];
        assert!(validate_cross_document_xrefs(Path::new("."), &valid).is_ok());

        let generated_heading = [
            document("index.adoc", "= 入口\n\nxref:guide.adoc#_details[詳細]\n"),
            document("guide.adoc", "= 案内\n\n== Details\n"),
        ];
        assert!(
            validate_cross_document_xrefs(Path::new("."), &generated_heading)
                .expect_err("generated heading ID must not be stable contract")
                .contains("明示ID")
        );

        let missing_anchor = [
            document("index.adoc", "= 入口\n\nxref:guide.adoc[案内]\n"),
            document("guide.adoc", "= 案内\n"),
        ];
        assert!(
            validate_cross_document_xrefs(Path::new("."), &missing_anchor)
                .expect_err("cross-document xref must name an explicit ID")
                .contains("明示IDを指定")
        );
    }

    #[test]
    fn xref_cannot_escape_the_project_or_omit_a_corpus_document() {
        let outside = [document(
            "docs/index.adoc",
            "= 入口\n\nxref:../../outside.adoc#target[外部]\n",
        )];
        assert!(
            validate_cross_document_xrefs(Path::new("."), &outside)
                .expect_err("outside xref")
                .contains("project rootの外")
        );

        let absent = [document(
            "docs/index.adoc",
            "= 入口\n\nxref:missing.adoc#target[不在]\n",
        )];
        assert!(
            validate_cross_document_xrefs(Path::new("."), &absent)
                .expect_err("missing corpus document")
                .contains("検査対象にありません")
        );
    }

    #[test]
    fn link_macros_must_not_remain_unrecognized_in_prose() {
        let invalid = [document(
            "index.adoc",
            "= 入口\n\n詳しくはxref:guide.adoc#details[詳細]を参照します。\n",
        )];
        assert!(
            validate_cross_document_xrefs(Path::new("."), &invalid)
                .expect_err("unrecognized xref must be rejected")
                .contains("通常の文章として解釈")
        );

        let literal = [document(
            "index.adoc",
            "= 入口\n\n``xref:guide.adoc#details[詳細]``という記法です。\n",
        )];
        assert!(validate_cross_document_xrefs(Path::new("."), &literal).is_ok());
    }

    #[test]
    fn repository_documents_reject_passthrough_markup() {
        let document = [document("index.adoc", "= 入口\n\n++**++強調++**++\n")];
        assert!(
            validate_cross_document_xrefs(Path::new("."), &document)
                .expect_err("passthrough must be rejected")
                .contains("passthrough記法")
        );
    }

    #[test]
    fn repository_documents_require_explicit_table_columns() {
        let document = [document(
            "index.adoc",
            "= 入口\n\n[cols=\",,\"]\n|===\n|一\n|二\n|三\n|===\n",
        )];
        assert!(
            validate_cross_document_xrefs(Path::new("."), &document)
                .expect_err("implicit empty table columns must be rejected")
                .contains("各列を明示")
        );
    }
}
