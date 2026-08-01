//! ノート操作の業務処理と、外側の実装に要求するport。

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use marginalis_domain::{
    Actor, BibliographyItem, Identity, MAX_GRAPH_DEPTH, Note, NoteAccess, NoteAclEntry, NoteDraft,
    NoteId, NoteListEntry, NoteSummary, NoteValidationTarget, Revision, UnixMillis, Utf8ByteSpan,
};

use crate::{
    BibliographyRepository, BibliographyRepositoryError, CitationStyle, Clock, NoteAccessControl,
    NoteAclChange, NoteAclState, NoteCommands, NotePresentation, NotePreview, NoteProfile,
    NoteQueries, NoteRenderContext, NoteUseCaseError, NoteValidationCode, NoteValidationDiagnostic,
    NoteWritePolicy, Random, RelatedNotes, ValidatedNoteDraft,
};

/// 永続化方式に依存しないrepositoryの失敗理由。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NoteRepositoryError {
    #[error("note was not found")]
    NotFound,
    #[error("note revision conflicts")]
    Conflict,
    #[error("stored note data is invalid")]
    CorruptData,
    #[error("note storage is unavailable")]
    Unavailable,
}

/// 可視性を適用してノートを読み取るport。
#[async_trait]
pub trait NoteQueryRepository: Send + Sync {
    async fn list_visible_notes(
        &self,
        actor: &Actor,
    ) -> Result<Vec<NoteListEntry>, NoteRepositoryError>;
    async fn accessible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<AccessibleNote>, NoteRepositoryError>;
    async fn visible_notes_by_id(
        &self,
        actor: &Actor,
        note_ids: &[NoteId],
    ) -> Result<Vec<Note>, NoteRepositoryError>;
    async fn note_view_snapshot(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<NoteViewSnapshot>, NoteRepositoryError>;
    /// 閲覧できるノートと、それらが引用する文献の関係を1回の読み取りで返す。
    async fn note_graph(
        &self,
        actor: &Actor,
        query: &NoteGraphQuery,
    ) -> Result<NoteGraph, NoteRepositoryError>;
}

/// 現在の利用者が閲覧できるノートと、その利用者に対する実効アクセス水準。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessibleNote {
    pub note: Note,
    pub access: NoteAccess,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteViewSnapshot {
    pub note: Note,
    pub access: NoteAccess,
    pub reference_targets: Vec<Note>,
    pub related: RelatedNotes,
}

/// 認可、revision、削除状態を一つのtransactionへ拘束する変更port。
#[async_trait]
pub trait NoteCommandRepository: Send + Sync {
    async fn create_note(
        &self,
        note: &Note,
        links: NoteLinks<'_>,
    ) -> Result<(), NoteRepositoryError>;
    #[allow(clippy::too_many_arguments)]
    async fn update_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        draft: &NoteDraft,
        links: NoteLinks<'_>,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
    async fn soft_delete_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
    async fn restore_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
}

/// 所有者だけが利用できるACL操作port。
#[async_trait]
pub trait NoteAclRepository: Send + Sync {
    async fn read_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<NoteAclState, NoteRepositoryError>;
    async fn replace_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
        entries: &[NoteAclEntry],
        expected_revision: Revision,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
}

/// 関係の図に出す点と線。
///
/// 点は現在の利用者が閲覧できるノートと、そのノートが引用している文献だけとする。線は始点と
/// 終点の両方が点として出る場合だけ返す。閲覧できないノートの存在も件数も現れない。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NoteGraph {
    pub notes: Vec<NoteGraphNote>,
    pub works: Vec<NoteGraphWork>,
    pub references: Vec<NoteGraphReference>,
    pub citations: Vec<NoteGraphCitation>,
}

/// 図に出すノート。本文は含めない。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteGraphNote {
    pub note_id: NoteId,
    pub title: String,
    pub tags: Vec<String>,
    pub updated_at: UnixMillis,
}

/// 図に出す文献。書誌ライブラリーの内容ではなく、引用されたという事実だけを表す。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteGraphWork {
    pub citation_key: String,
    /// 引用元のノートを書いた利用者のライブラリーで解決できた場合の題名。
    pub title: Option<String>,
}

/// ノートからノートへの参照。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteGraphReference {
    pub source_note_id: NoteId,
    pub target_note_id: NoteId,
}

/// ノートから文献への引用。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteGraphCitation {
    pub source_note_id: NoteId,
    pub citation_key: String,
}

/// 図に出す範囲の指定。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NoteGraphQuery {
    /// 題名、本文、タグのいずれかにこの語を含むノートだけへ絞る。
    pub text: Option<String>,
    /// 起点のノート。指定すると、そこから[`NoteGraphQuery::depth`]階層以内だけを残す。
    pub origin: Option<NoteId>,
    /// 起点から数えて何本の線を辿るか。起点を指定しない場合は使わない。
    pub depth: Option<u32>,
}

impl NoteGraph {
    /// 起点から`depth`本以内の線で辿れる点と、その間の線だけを残す。
    ///
    /// 認可は問い合わせの側で済んでいる。ここで扱うのは、閲覧できる範囲のうちどこを見せるかと
    /// いう表示上の絞り込みだけである。起点が図に無い場合は空の図を返す。
    pub fn within(self, origin: NoteId, depth: u32) -> Self {
        let works = |key: &str| format!("work:{key}");
        let note_key = |note_id: NoteId| format!("note:{note_id}");
        if !self.notes.iter().any(|note| note.note_id == origin) {
            return Self::default();
        }

        // 参照と引用をどちらも双方向に辿る。向きを問わないのは、離れた話題のつながりを
        // 見つけることが図の目的だからである。
        let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
        let mut join = |left: String, right: String| {
            adjacency
                .entry(left.clone())
                .or_default()
                .push(right.clone());
            adjacency.entry(right).or_default().push(left);
        };
        for edge in &self.references {
            join(note_key(edge.source_note_id), note_key(edge.target_note_id));
        }
        for edge in &self.citations {
            join(note_key(edge.source_note_id), works(&edge.citation_key));
        }

        let mut reached: HashSet<String> = HashSet::new();
        let mut frontier = vec![note_key(origin)];
        reached.insert(note_key(origin));
        for _ in 0..depth.min(MAX_GRAPH_DEPTH) {
            let mut next = Vec::new();
            for vertex in frontier {
                for neighbour in adjacency.get(&vertex).into_iter().flatten() {
                    if reached.insert(neighbour.clone()) {
                        next.push(neighbour.clone());
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }

        Self {
            notes: self
                .notes
                .into_iter()
                .filter(|note| reached.contains(&note_key(note.note_id)))
                .collect(),
            works: self
                .works
                .into_iter()
                .filter(|work| reached.contains(&works(&work.citation_key)))
                .collect(),
            references: self
                .references
                .into_iter()
                .filter(|edge| {
                    reached.contains(&note_key(edge.source_note_id))
                        && reached.contains(&note_key(edge.target_note_id))
                })
                .collect(),
            citations: self
                .citations
                .into_iter()
                .filter(|edge| {
                    reached.contains(&note_key(edge.source_note_id))
                        && reached.contains(&works(&edge.citation_key))
                })
                .collect(),
        }
    }
}

/// 本文から導いた、ノートが指し示す先の一覧。
///
/// ノート参照と引用は、どちらも本文の解析から得て同じtransactionで置き換える。別々のport
/// 引数にすると、片方だけ渡し忘れても型が通ってしまう。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoteLinks<'a> {
    pub reference_targets: &'a [NoteId],
    pub cited_keys: &'a [String],
}

/// 文書内で見つかったノート参照。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteReferenceQuery {
    pub reference_index: usize,
    pub target_note_id: NoteId,
    pub anchor: Option<String>,
}

/// 認可と外部URLの解決を終えたノート参照。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoteReferenceResolution {
    Visible {
        reference_index: usize,
        href: String,
        title: String,
        missing_anchor: bool,
    },
    Hidden {
        reference_index: usize,
    },
}

/// 本文が書誌ライブラリーへ問い合わせる引用1件。
///
/// `cite:[a, b]`のように1つの引用が複数のcitation keyを名指すため、keyは並びで持つ。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteCitationQuery {
    pub citation_index: usize,
    pub keys: Vec<String>,
    /// `locator="p. 12"`のように引用へ添えられた位置。
    pub locator: Option<String>,
    /// 本文中で引用が占める範囲。診断の位置に使う。
    pub span: Utf8ByteSpan,
}

/// 書誌ライブラリーで解決を終えた引用の表示。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteCitationResolution {
    pub citation_index: usize,
    pub segments: Vec<NoteCitationSegment>,
}

/// 引用表示のうち、link先を共有する連続した一区切り。
///
/// `(Smith 2024; Tanaka 2025)`のように、括弧と区切りは素の文字列のまま、著者名だけを
/// 参考文献項目へlinkさせるために分ける。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteCitationSegment {
    pub text: String,
    /// link先の参考文献項目のanchor。`None`は素の文字列として表示する。
    pub anchor: Option<String>,
}

/// 本文の末尾へ生成する参考文献一覧の1項目。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteBibliographyEntry {
    pub citation_key: String,
    pub text: String,
    /// 項目の見出しとして表示する短い文字列。
    ///
    /// 番号で示すスタイルでは初出順の番号が入ります。AsciiDocはbibliography anchorの
    /// カンマ以降を表示テキストとして読み、項目と本文からの参照を`[表示テキスト]`の形に
    /// します。`None`の場合はcitation keyがそのまま見出しになります。
    pub label: Option<String>,
}

/// 描画時に文書adapterへ渡す解決結果一式。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoteRenderInputs<'a> {
    pub references: &'a [NoteReferenceResolution],
    pub citations: &'a [NoteCitationResolution],
    pub bibliography: &'a [NoteBibliographyEntry],
}

/// 文書adapterが保存済みの内容を解析または変換できない場合の失敗。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("note content could not be processed")]
pub struct NoteContentError;

/// AsciiDocなどの文書形式に依存する処理を受け持つport。
pub trait NoteContent: Send + Sync {
    fn validate_draft(
        &self,
        draft: NoteDraft,
    ) -> Result<ValidatedNoteDraft, Vec<NoteValidationDiagnostic>>;
    fn reference_queries(&self, body: &str) -> Result<Vec<NoteReferenceQuery>, NoteContentError>;
    fn citation_queries(&self, body: &str) -> Result<Vec<NoteCitationQuery>, NoteContentError>;
    /// 本文のheaderが選んだ引用の表示規則を返す。
    ///
    /// 保存済みのノートを表示するときは下書きの検証結果が手元にないため、本文から読み直す。
    fn citation_style(&self, body: &str) -> Result<CitationStyle, NoteContentError>;
    fn has_anchor(&self, body: &str, anchor: &str) -> Result<bool, NoteContentError>;
    fn render(&self, note: &Note, inputs: NoteRenderInputs<'_>)
    -> Result<String, NoteContentError>;
    fn export(&self, note: &Note) -> Result<String, NoteContentError>;
    fn profile(&self) -> NoteProfile;
}

/// HTTPの配置方式に依存するノートURLを組み立てるport。
pub trait NoteLinkResolver: Send + Sync {
    fn href(
        &self,
        context: &NoteRenderContext,
        note_id: NoteId,
        anchor: Option<&str>,
    ) -> Option<String>;
}

/// transportへ公開するノート操作のapplication service。
pub struct NoteApplication {
    queries: Arc<dyn NoteQueryRepository>,
    commands: Arc<dyn NoteCommandRepository>,
    access_control: Arc<dyn NoteAclRepository>,
    content: Arc<dyn NoteContent>,
    bibliography: Arc<dyn BibliographyRepository>,
    links: Arc<dyn NoteLinkResolver>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn Random>,
}

impl NoteApplication {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        queries: Arc<dyn NoteQueryRepository>,
        commands: Arc<dyn NoteCommandRepository>,
        access_control: Arc<dyn NoteAclRepository>,
        content: Arc<dyn NoteContent>,
        bibliography: Arc<dyn BibliographyRepository>,
        links: Arc<dyn NoteLinkResolver>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
    ) -> Self {
        Self {
            queries,
            commands,
            access_control,
            content,
            bibliography,
            links,
            clock,
            random,
        }
    }

    async fn read_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Note, NoteUseCaseError> {
        self.queries
            .accessible_note(actor, note_id)
            .await
            .map_err(map_repository_error)?
            .map(|accessible| accessible.note)
            .ok_or(NoteUseCaseError::NotFound)
    }

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
        } = validated;
        let note = Note::create(note_id, owner, draft, self.clock.now());
        let target_ids = reference_targets(&reference_queries);
        let targets = self
            .queries
            .visible_notes_by_id(actor, &target_ids)
            .await
            .map_err(map_repository_error)?;
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
        Ok(NotePreview { html, diagnostics })
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

    /// 引用のcitation keyを、ノートを書いた利用者の書誌ライブラリーで解決する。
    ///
    /// 閲覧者ではなく作成者のライブラリーを使うため、同じノートは誰が見ても同じ表示になる。
    /// 解決できたkeyだけが参考文献一覧へ並び、同じ文献を何度引用しても項目は1つになる。
    async fn citation_resolutions(
        &self,
        owner: &Identity,
        queries: &[NoteCitationQuery],
        style: CitationStyle,
    ) -> Result<ResolvedCitations, NoteUseCaseError> {
        if queries.is_empty() {
            return Ok(ResolvedCitations::default());
        }
        let mut cited_keys = Vec::new();
        for key in queries.iter().flat_map(|query| query.keys.iter()) {
            if !cited_keys.contains(key) {
                cited_keys.push(key.clone());
            }
        }
        let items = self
            .bibliography
            .items_by_citation_keys(owner, &cited_keys)
            .await
            .map_err(|error| match error {
                BibliographyRepositoryError::CorruptData => NoteUseCaseError::CorruptData,
                _ => NoteUseCaseError::Unavailable,
            })?;
        let items = items
            .into_iter()
            .map(|item| (item.citation_key().to_owned(), item))
            .collect::<HashMap<_, _>>();

        // 番号で示すスタイルは、本文での初出順に通し番号を振る。解決できたkeyだけが一覧へ
        // 並ぶため、番号も解決できたkeyの中で数える。
        let numbers = cited_keys
            .iter()
            .filter(|key| items.contains_key(*key))
            .enumerate()
            .map(|(position, key)| (key.clone(), position + 1))
            .collect::<HashMap<_, _>>();
        let resolutions = queries
            .iter()
            .map(|query| NoteCitationResolution {
                citation_index: query.citation_index,
                segments: citation_segments(query, &items, &numbers, style),
            })
            .collect();
        let entries = cited_keys
            .iter()
            .filter_map(|key| {
                let item = items.get(key)?;
                Some(NoteBibliographyEntry {
                    citation_key: key.clone(),
                    text: style.entry_text(item),
                    label: style.entry_label(numbers[key]),
                })
            })
            .collect();
        let unknown_keys = cited_keys
            .iter()
            .filter(|key| !items.contains_key(*key))
            .cloned()
            .collect::<Vec<_>>();
        Ok(ResolvedCitations {
            resolutions,
            entries,
            diagnostics: unknown_citation_diagnostics(queries, &unknown_keys),
        })
    }
}

/// 1つのノートについて解決した引用の表示、参考文献項目、保存を妨げない診断。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ResolvedCitations {
    resolutions: Vec<NoteCitationResolution>,
    entries: Vec<NoteBibliographyEntry>,
    diagnostics: Vec<crate::NoteAdvisoryDiagnostic>,
}

/// 引用1件の表示を、括弧と区切りを含む文字列の並びへ組み立てる。
///
/// 解決できたkeyは参考文献項目のanchorへlinkし、解決できなかったkeyはcitation keyを
/// そのまま表示する。定義のない`<<key>>`と同じ見え方にそろえ、値を推測しない。
fn citation_segments(
    query: &NoteCitationQuery,
    items: &HashMap<String, BibliographyItem>,
    numbers: &HashMap<String, usize>,
    style: CitationStyle,
) -> Vec<NoteCitationSegment> {
    let (opening, closing) = style.brackets();
    let mut segments = vec![NoteCitationSegment {
        text: opening.into(),
        anchor: None,
    }];
    for (position, key) in query.keys.iter().enumerate() {
        if position > 0 {
            segments.push(NoteCitationSegment {
                text: style.key_separator().into(),
                anchor: None,
            });
        }
        match items.get(key) {
            Some(item) => segments.push(NoteCitationSegment {
                text: style.inline_label(item, numbers[key]),
                anchor: Some(key.clone()),
            }),
            None => segments.push(NoteCitationSegment {
                text: key.clone(),
                anchor: None,
            }),
        }
    }
    let closing = match query.locator.as_deref() {
        Some(locator) => format!(", {locator}{closing}"),
        None => closing.into(),
    };
    segments.push(NoteCitationSegment {
        text: closing,
        anchor: None,
    });
    segments
}

/// 書誌ライブラリーに無いcitation keyを、保存を妨げない警告として報告する。
fn unknown_citation_diagnostics(
    queries: &[NoteCitationQuery],
    unknown_keys: &[String],
) -> Vec<crate::NoteAdvisoryDiagnostic> {
    queries
        .iter()
        .filter_map(|query| {
            let missing = query
                .keys
                .iter()
                .filter(|key| unknown_keys.contains(key))
                .cloned()
                .collect::<Vec<_>>();
            (!missing.is_empty()).then(|| crate::NoteAdvisoryDiagnostic {
                code: "unknown_citation_key".into(),
                severity: crate::NoteAdvisorySeverity::Warning,
                target: NoteValidationTarget::Source,
                span: Some(query.span),
                message: format!(
                    "the bibliography library has no item for {}",
                    missing.join(", ")
                ),
            })
        })
        .collect()
}

#[async_trait]
impl NoteQueries for NoteApplication {
    async fn list_visible_notes(
        &self,
        actor: Actor,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        self.queries
            .list_visible_notes(&actor)
            .await
            .map_err(map_repository_error)
    }

    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        self.read_visible_note(&actor, note_id).await
    }
}

#[async_trait]
impl NoteCommands for NoteApplication {
    async fn create_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError> {
        let validated = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        let ValidatedNoteDraft {
            draft,
            mut diagnostics,
            reference_queries,
            citation_queries,
            citation_style,
        } = validated;
        // 新規作成では操作している利用者がそのまま作成者になるため、閲覧時の解決先と一致する。
        diagnostics.extend(
            self.citation_resolutions(actor.identity(), &citation_queries, citation_style)
                .await?
                .diagnostics,
        );
        reject_warnings(policy, diagnostics)?;
        let now = self.clock.now();
        let note = Note::create(
            NoteId::new(self.random.uuid_v7()),
            actor.identity(),
            draft,
            now,
        );
        let reference_targets = reference_targets(&reference_queries);
        let cited_keys = cited_keys(&citation_queries);
        self.commands
            .create_note(
                &note,
                NoteLinks {
                    reference_targets: &reference_targets,
                    cited_keys: &cited_keys,
                },
            )
            .await
            .map_err(map_repository_error)?;
        Ok(note)
    }

    async fn update_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        expected_revision: Revision,
        policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError> {
        let validated = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        let ValidatedNoteDraft {
            draft,
            mut diagnostics,
            reference_queries,
            citation_queries,
            citation_style,
        } = validated;
        if !citation_queries.is_empty() {
            // 引用は閲覧時に作成者のライブラリーで解決する。共有されたノートを別の利用者が
            // 更新する場合も同じ基準で判定しないと、保存できた引用が表示では解決されない。
            let owner = self
                .read_visible_note(&actor, note_id)
                .await?
                .owner()
                .clone();
            diagnostics.extend(
                self.citation_resolutions(&owner, &citation_queries, citation_style)
                    .await?
                    .diagnostics,
            );
        }
        reject_warnings(policy, diagnostics)?;
        let reference_targets = reference_targets(&reference_queries);
        let cited_keys = cited_keys(&citation_queries);
        self.commands
            .update_visible_note(
                &actor,
                note_id,
                expected_revision,
                &draft,
                NoteLinks {
                    reference_targets: &reference_targets,
                    cited_keys: &cited_keys,
                },
                self.clock.now(),
            )
            .await
            .map_err(map_repository_error)
    }

    async fn soft_delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        self.commands
            .soft_delete_visible_note(&actor, note_id, expected_revision, self.clock.now())
            .await
            .map_err(map_repository_error)
    }

    async fn restore_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        self.commands
            .restore_visible_note(&actor, note_id, expected_revision, self.clock.now())
            .await
            .map_err(map_repository_error)
    }
}

fn reject_warnings(
    policy: NoteWritePolicy,
    diagnostics: Vec<crate::NoteAdvisoryDiagnostic>,
) -> Result<(), NoteUseCaseError> {
    if policy == NoteWritePolicy::RejectWarnings
        && diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::NoteAdvisorySeverity::Warning)
    {
        return Err(NoteUseCaseError::AdvisoriesRejected(diagnostics));
    }
    Ok(())
}

#[async_trait]
impl NotePresentation for NoteApplication {
    async fn preview_new_note(
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

    async fn preview_note_update(
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
            .map_err(map_repository_error)?
            .filter(|accessible| accessible.access.allows(NoteAccess::Edit))
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

    fn export_note_source(&self, note: &Note) -> Result<String, NoteUseCaseError> {
        self.content
            .export(note)
            .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn read_note_graph(
        &self,
        actor: Actor,
        query: NoteGraphQuery,
    ) -> Result<NoteGraph, NoteUseCaseError> {
        let graph = self
            .queries
            .note_graph(&actor, &query)
            .await
            .map_err(map_repository_error)?;
        // 起点からの絞り込みは、閲覧できる範囲が確定した後で行う。認可の判断はrepositoryが
        // 済ませており、ここで扱うのは表示範囲だけである。
        Ok(match query.origin {
            Some(origin) => graph.within(origin, query.depth.unwrap_or(1)),
            None => graph,
        })
    }

    fn note_profile(&self) -> NoteProfile {
        self.content.profile()
    }

    async fn read_note_view(
        &self,
        actor: Actor,
        note_id: NoteId,
        context: NoteRenderContext,
    ) -> Result<crate::NoteView, NoteUseCaseError> {
        let mut snapshot = self
            .queries
            .note_view_snapshot(&actor, note_id)
            .await
            .map_err(map_repository_error)?
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
        Ok(crate::NoteView {
            note: snapshot.note,
            access: snapshot.access,
            html,
            related: snapshot.related,
        })
    }
}

#[async_trait]
impl NoteAccessControl for NoteApplication {
    async fn read_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteAclState, NoteUseCaseError> {
        self.access_control
            .read_note_acl(&actor, note_id)
            .await
            .map_err(map_repository_error)
    }

    async fn replace_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
        mut entries: Vec<NoteAclChange>,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        let note = self.read_visible_note(&actor, note_id).await?;
        entries.sort_by(|left, right| left.subject.cmp(&right.subject));
        let mut grants = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let identity =
                Identity::new(note.creator_issuer().to_owned(), entry.subject.clone())
                    .map_err(|_| acl_validation(index, AclValidationProblem::InvalidSubject))?;
            if entry.subject == note.creator_subject() {
                return Err(acl_validation(index, AclValidationProblem::OwnerIncluded));
            }
            if index > 0 && entries[index - 1].subject == entry.subject {
                return Err(acl_validation(
                    index,
                    AclValidationProblem::DuplicateSubject,
                ));
            }
            grants.push(NoteAclEntry::new(identity, entry.permission));
        }
        self.access_control
            .replace_note_acl(
                &actor,
                note_id,
                &grants,
                expected_revision,
                self.clock.now(),
            )
            .await
            .map_err(map_repository_error)
    }
}

fn map_repository_error(error: NoteRepositoryError) -> NoteUseCaseError {
    match error {
        NoteRepositoryError::NotFound => NoteUseCaseError::NotFound,
        NoteRepositoryError::Conflict => NoteUseCaseError::Conflict,
        NoteRepositoryError::CorruptData => NoteUseCaseError::CorruptData,
        NoteRepositoryError::Unavailable => NoteUseCaseError::Unavailable,
    }
}

/// ACL入力だけで起こる問題。
///
/// 全体の`NoteValidationCode`を受け取ると、ACLでは起こらない値も型の上では渡せてしまう。
/// 到達しないことをコメントで主張せず、渡せる値を型で限定する。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AclValidationProblem {
    InvalidSubject,
    DuplicateSubject,
    OwnerIncluded,
}

impl AclValidationProblem {
    const fn code(self) -> NoteValidationCode {
        match self {
            Self::InvalidSubject => NoteValidationCode::InvalidAclSubject,
            Self::DuplicateSubject => NoteValidationCode::DuplicateAclSubject,
            Self::OwnerIncluded => NoteValidationCode::OwnerInAcl,
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::InvalidSubject => "ACL subject is invalid",
            Self::DuplicateSubject => "ACL subject is duplicated",
            Self::OwnerIncluded => "note owner must not be included in ACL",
        }
    }
}

fn acl_validation(index: usize, problem: AclValidationProblem) -> NoteUseCaseError {
    NoteUseCaseError::Validation(vec![NoteValidationDiagnostic {
        code: problem.code().as_str().into(),
        target: NoteValidationTarget::AclEntry { index },
        span: None,
        message: problem.message().into(),
    }])
}

/// 本文が名指したcitation keyを、重複なく並べる。
///
/// 書誌ライブラリーに実在するかどうかは問わない。ライブラリーは後から変わるため、保存する
/// のは「本文が何を引用したか」であって「解決できたか」ではない。
fn cited_keys(queries: &[NoteCitationQuery]) -> Vec<String> {
    let mut keys = queries
        .iter()
        .flat_map(|query| query.keys.iter().cloned())
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys
}

fn reference_targets(queries: &[NoteReferenceQuery]) -> Vec<NoteId> {
    queries
        .iter()
        .map(|query| query.target_note_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
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
    use std::{
        str::FromStr,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use marginalis_domain::{EntityId, Revision, UnixMillis};

    use crate::{NoteAdvisoryDiagnostic, NoteAdvisorySeverity};

    use super::*;

    struct MemoryNotes {
        notes: Mutex<Vec<Note>>,
        update_calls: AtomicUsize,
        accessible_as: Mutex<Option<NoteAccess>>,
    }

    impl Default for MemoryNotes {
        fn default() -> Self {
            Self {
                notes: Mutex::new(Vec::new()),
                update_calls: AtomicUsize::new(0),
                accessible_as: Mutex::new(Some(NoteAccess::Manage)),
            }
        }
    }

    #[async_trait]
    impl NoteQueryRepository for MemoryNotes {
        async fn list_visible_notes(
            &self,
            _actor: &Actor,
        ) -> Result<Vec<NoteListEntry>, NoteRepositoryError> {
            Ok(self
                .notes
                .lock()
                .expect("notes lock")
                .iter()
                .map(|note| NoteListEntry {
                    summary: NoteSummary::from(note),
                    access: NoteAccess::Manage,
                })
                .collect())
        }

        async fn accessible_note(
            &self,
            _actor: &Actor,
            note_id: NoteId,
        ) -> Result<Option<AccessibleNote>, NoteRepositoryError> {
            let access = *self.accessible_as.lock().expect("access lock");
            Ok(access.and_then(|access| {
                self.notes
                    .lock()
                    .expect("notes lock")
                    .iter()
                    .find(|note| note.note_id() == note_id)
                    .cloned()
                    .map(|note| AccessibleNote { note, access })
            }))
        }

        async fn visible_notes_by_id(
            &self,
            actor: &Actor,
            note_ids: &[NoteId],
        ) -> Result<Vec<Note>, NoteRepositoryError> {
            let mut notes = Vec::new();
            for note_id in note_ids {
                if let Some(accessible) = self.accessible_note(actor, *note_id).await? {
                    notes.push(accessible.note);
                }
            }
            Ok(notes)
        }

        async fn note_view_snapshot(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
        ) -> Result<Option<NoteViewSnapshot>, NoteRepositoryError> {
            Ok(None)
        }

        async fn note_graph(
            &self,
            _actor: &Actor,
            _query: &NoteGraphQuery,
        ) -> Result<NoteGraph, NoteRepositoryError> {
            Ok(NoteGraph::default())
        }
    }

    #[async_trait]
    impl NoteCommandRepository for MemoryNotes {
        async fn create_note(
            &self,
            note: &Note,
            _links: NoteLinks<'_>,
        ) -> Result<(), NoteRepositoryError> {
            self.notes.lock().expect("notes lock").push(note.clone());
            Ok(())
        }

        async fn update_visible_note(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
            _expected_revision: Revision,
            _draft: &NoteDraft,
            _links: NoteLinks<'_>,
            _now: UnixMillis,
        ) -> Result<Note, NoteRepositoryError> {
            self.update_calls.fetch_add(1, Ordering::Relaxed);
            Err(NoteRepositoryError::Unavailable)
        }

        async fn soft_delete_visible_note(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
            _expected_revision: Revision,
            _now: UnixMillis,
        ) -> Result<Note, NoteRepositoryError> {
            Err(NoteRepositoryError::Unavailable)
        }

        async fn restore_visible_note(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
            _expected_revision: Revision,
            _now: UnixMillis,
        ) -> Result<Note, NoteRepositoryError> {
            Err(NoteRepositoryError::Unavailable)
        }
    }

    #[async_trait]
    impl NoteAclRepository for MemoryNotes {
        async fn read_note_acl(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
        ) -> Result<NoteAclState, NoteRepositoryError> {
            Ok(NoteAclState {
                entries: Vec::new(),
                revision: Revision::INITIAL,
            })
        }

        async fn replace_note_acl(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
            _entries: &[NoteAclEntry],
            _expected_revision: Revision,
            _now: UnixMillis,
        ) -> Result<Note, NoteRepositoryError> {
            Err(NoteRepositoryError::Unavailable)
        }
    }

    #[derive(Default)]
    struct AcceptContent {
        reference_query_calls: AtomicUsize,
    }

    impl NoteContent for AcceptContent {
        fn validate_draft(
            &self,
            draft: NoteDraft,
        ) -> Result<ValidatedNoteDraft, Vec<NoteValidationDiagnostic>> {
            Ok(ValidatedNoteDraft {
                draft,
                diagnostics: vec![NoteAdvisoryDiagnostic {
                    code: "test-advisory".into(),
                    severity: NoteAdvisorySeverity::Warning,
                    target: NoteValidationTarget::Source,
                    span: None,
                    message: "test advisory".into(),
                }],
                reference_queries: Vec::new(),
                citation_queries: Vec::new(),
                citation_style: CitationStyle::default(),
            })
        }

        fn reference_queries(
            &self,
            _body: &str,
        ) -> Result<Vec<NoteReferenceQuery>, NoteContentError> {
            self.reference_query_calls.fetch_add(1, Ordering::Relaxed);
            Ok(Vec::new())
        }

        fn citation_queries(
            &self,
            _body: &str,
        ) -> Result<Vec<NoteCitationQuery>, NoteContentError> {
            Ok(Vec::new())
        }

        fn citation_style(&self, _body: &str) -> Result<CitationStyle, NoteContentError> {
            Ok(CitationStyle::default())
        }

        fn has_anchor(&self, _body: &str, _anchor: &str) -> Result<bool, NoteContentError> {
            Ok(false)
        }

        fn render(
            &self,
            _note: &Note,
            _inputs: NoteRenderInputs<'_>,
        ) -> Result<String, NoteContentError> {
            Ok("<article><p>preview</p></article>".into())
        }

        fn export(&self, _note: &Note) -> Result<String, NoteContentError> {
            Ok(String::new())
        }

        fn profile(&self) -> NoteProfile {
            unreachable!("this test does not read the authoring profile")
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> UnixMillis {
            UnixMillis::new(1_700_000_000_000)
        }
    }

    struct FixedRandom;

    impl Random for FixedRandom {
        fn uuid_v7(&self) -> EntityId {
            EntityId::from_str("01890f3c-6a4d-7cc2-98b3-84b68f68c6e1").expect("fixed UUIDv7")
        }

        fn opaque_token(&self) -> String {
            unreachable!("note creation does not issue an opaque token")
        }
    }

    /// 引用のないノートだけを扱う試験用の書誌ライブラリー。
    struct EmptyLibrary;

    #[async_trait]
    impl BibliographyRepository for EmptyLibrary {
        async fn search_owned_items(
            &self,
            _actor: &Actor,
            _query: &str,
        ) -> Result<Vec<BibliographyItem>, BibliographyRepositoryError> {
            Ok(Vec::new())
        }

        async fn items_by_citation_keys(
            &self,
            _owner: &Identity,
            _citation_keys: &[String],
        ) -> Result<Vec<BibliographyItem>, BibliographyRepositoryError> {
            Ok(Vec::new())
        }

        async fn create_owned_item(
            &self,
            _item: &BibliographyItem,
        ) -> Result<(), BibliographyRepositoryError> {
            unreachable!("this test does not write bibliography items")
        }

        async fn update_owned_item(
            &self,
            _actor: &Actor,
            _item_id: marginalis_domain::BibliographyItemId,
            _citation_key: &str,
            _csl_json: &str,
            _updated_at: UnixMillis,
            _expected_revision: Revision,
        ) -> Result<BibliographyItem, BibliographyRepositoryError> {
            unreachable!("this test does not write bibliography items")
        }

        async fn delete_owned_item(
            &self,
            _actor: &Actor,
            _item_id: marginalis_domain::BibliographyItemId,
            _expected_revision: Revision,
        ) -> Result<(), BibliographyRepositoryError> {
            unreachable!("this test does not write bibliography items")
        }
    }

    struct NoLinks;

    impl NoteLinkResolver for NoLinks {
        fn href(
            &self,
            _context: &NoteRenderContext,
            _note_id: NoteId,
            _anchor: Option<&str>,
        ) -> Option<String> {
            None
        }
    }

    /// 1件だけ登録された書誌ライブラリー。所有者が一致する問い合わせにだけ答える。
    struct OneItemLibrary;

    impl OneItemLibrary {
        fn owner() -> Identity {
            Identity::new("https://id.example.test".into(), "alice".into()).expect("owner")
        }

        fn item() -> BibliographyItem {
            BibliographyItem::create(
                marginalis_domain::BibliographyItemId::new(
                    EntityId::from_str("0197c9bc-0000-7000-8000-000000000021").expect("UUIDv7"),
                ),
                &Self::owner(),
                "smith2024".into(),
                serde_json::json!({
                    "id": "smith2024",
                    "type": "article-journal",
                    "title": "An Example Article",
                    "author": [{ "family": "Smith", "given": "Alex" }],
                    "issued": { "date-parts": [[2024]] }
                })
                .to_string(),
                UnixMillis::new(0),
            )
        }

        /// 番号で示すスタイルの通し番号を確かめるための2件目。
        fn second_item() -> BibliographyItem {
            BibliographyItem::create(
                marginalis_domain::BibliographyItemId::new(
                    EntityId::from_str("0197c9bc-0000-7000-8000-000000000022").expect("UUIDv7"),
                ),
                &Self::owner(),
                "tanaka2025".into(),
                serde_json::json!({
                    "id": "tanaka2025",
                    "type": "article-journal",
                    "title": "追試の報告",
                    "author": [{ "family": "Tanaka", "given": "Bun" }],
                    "issued": { "date-parts": [[2025]] }
                })
                .to_string(),
                UnixMillis::new(0),
            )
        }
    }

    #[async_trait]
    impl BibliographyRepository for OneItemLibrary {
        async fn search_owned_items(
            &self,
            _actor: &Actor,
            _query: &str,
        ) -> Result<Vec<BibliographyItem>, BibliographyRepositoryError> {
            Ok(Vec::new())
        }

        async fn items_by_citation_keys(
            &self,
            owner: &Identity,
            citation_keys: &[String],
        ) -> Result<Vec<BibliographyItem>, BibliographyRepositoryError> {
            if owner != &Self::owner() {
                return Ok(Vec::new());
            }
            Ok([Self::item(), Self::second_item()]
                .into_iter()
                .filter(|item| citation_keys.iter().any(|key| key == item.citation_key()))
                .collect())
        }

        async fn create_owned_item(
            &self,
            _item: &BibliographyItem,
        ) -> Result<(), BibliographyRepositoryError> {
            unreachable!("this test does not write bibliography items")
        }

        async fn update_owned_item(
            &self,
            _actor: &Actor,
            _item_id: marginalis_domain::BibliographyItemId,
            _citation_key: &str,
            _csl_json: &str,
            _updated_at: UnixMillis,
            _expected_revision: Revision,
        ) -> Result<BibliographyItem, BibliographyRepositoryError> {
            unreachable!("this test does not write bibliography items")
        }

        async fn delete_owned_item(
            &self,
            _actor: &Actor,
            _item_id: marginalis_domain::BibliographyItemId,
            _expected_revision: Revision,
        ) -> Result<(), BibliographyRepositoryError> {
            unreachable!("this test does not write bibliography items")
        }
    }

    /// 引用だけを報告し、他の診断を出さない試験用の文書adapter。
    struct CitingContent {
        keys: Vec<String>,
    }

    impl NoteContent for CitingContent {
        fn validate_draft(
            &self,
            draft: NoteDraft,
        ) -> Result<ValidatedNoteDraft, Vec<NoteValidationDiagnostic>> {
            Ok(ValidatedNoteDraft {
                draft,
                diagnostics: Vec::new(),
                reference_queries: Vec::new(),
                citation_queries: vec![NoteCitationQuery {
                    citation_index: 0,
                    keys: self.keys.clone(),
                    locator: None,
                    span: Utf8ByteSpan { start: 0, end: 1 },
                }],
                citation_style: CitationStyle::default(),
            })
        }

        fn reference_queries(
            &self,
            _body: &str,
        ) -> Result<Vec<NoteReferenceQuery>, NoteContentError> {
            Ok(Vec::new())
        }

        fn citation_queries(
            &self,
            _body: &str,
        ) -> Result<Vec<NoteCitationQuery>, NoteContentError> {
            Ok(Vec::new())
        }

        fn citation_style(&self, _body: &str) -> Result<CitationStyle, NoteContentError> {
            Ok(CitationStyle::default())
        }

        fn has_anchor(&self, _body: &str, _anchor: &str) -> Result<bool, NoteContentError> {
            Ok(false)
        }

        fn render(
            &self,
            _note: &Note,
            _inputs: NoteRenderInputs<'_>,
        ) -> Result<String, NoteContentError> {
            Ok(String::new())
        }

        fn export(&self, _note: &Note) -> Result<String, NoteContentError> {
            Ok(String::new())
        }

        fn profile(&self) -> NoteProfile {
            unreachable!("this test does not read the authoring profile")
        }
    }

    /// 共有されたノートの更新でも、引用は作成者のライブラリーで判定する。
    ///
    /// 判定先が操作者のままだと、保存できた引用が閲覧時に解決されない状態を作ってしまう。
    #[tokio::test]
    async fn updating_a_shared_note_judges_citations_against_its_creator() {
        let repository = Arc::new(MemoryNotes::default());
        let note_id = NoteId::new(
            EntityId::from_str("0197c9bc-0000-7000-8000-000000000031").expect("UUIDv7"),
        );
        repository.notes.lock().expect("notes lock").push(
            Note::restore(
                note_id,
                OneItemLibrary::owner(),
                "共有されたノート".into(),
                "= 共有されたノート\n\n本文".into(),
                Vec::new(),
                UnixMillis::new(0),
                UnixMillis::new(1),
                Revision::INITIAL,
                None,
            )
            .expect("stored note"),
        );
        let application = NoteApplication::new(
            repository.clone(),
            repository.clone(),
            repository.clone(),
            Arc::new(CitingContent {
                keys: vec!["smith2024".into()],
            }),
            Arc::new(OneItemLibrary),
            Arc::new(NoLinks),
            Arc::new(FixedClock),
            Arc::new(FixedRandom),
        );
        // 作成者ではない編集者。自分のライブラリーには`smith2024`がない。
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
        assert!(
            preview.diagnostics.is_empty(),
            "更新用プレビューも保存時と同じ所有者の書誌ライブラリーを使います"
        );

        *repository.accessible_as.lock().expect("access lock") = Some(NoteAccess::Read);
        assert_eq!(
            application
                .preview_note_update(
                    editor.clone(),
                    note_id,
                    draft.clone(),
                    NoteRenderContext {
                        note_path_prefix: "/notes".into(),
                    },
                )
                .await,
            Err(NoteUseCaseError::NotFound)
        );
        *repository.accessible_as.lock().expect("access lock") = Some(NoteAccess::Edit);

        // 作成者のライブラリーで解決できるため、警告を拒否する方針でも書き込みまで進む。
        assert_eq!(
            application
                .update_note(
                    editor.clone(),
                    note_id,
                    draft.clone(),
                    Revision::INITIAL,
                    NoteWritePolicy::RejectWarnings,
                )
                .await,
            Err(NoteUseCaseError::Unavailable)
        );

        // 新規作成では操作者が作成者になるため、同じ引用は未登録として拒否される。
        let created = application
            .create_note(editor, draft, NoteWritePolicy::RejectWarnings)
            .await;
        let Err(NoteUseCaseError::AdvisoriesRejected(diagnostics)) = created else {
            panic!("未登録のcitation keyは新規作成で拒否されます: {created:?}");
        };
        assert_eq!(diagnostics[0].code, "unknown_citation_key");
    }

    /// 番号で示すスタイルは、本文での初出順に通し番号を振る。
    ///
    /// 同じ文献を何度引用しても番号は変わらず、参考文献一覧の項目も1つだけになる。番号は
    /// 一覧の項目にも付くため、本文の`[1]`から一覧の`[1]`を探せる。
    #[tokio::test]
    async fn the_numeric_style_numbers_citations_by_first_appearance() {
        let application = citation_application();
        let queries = vec![
            NoteCitationQuery {
                citation_index: 0,
                keys: vec!["smith2024".into()],
                locator: None,
                span: Utf8ByteSpan { start: 10, end: 30 },
            },
            NoteCitationQuery {
                citation_index: 1,
                keys: vec!["tanaka2025".into()],
                locator: None,
                span: Utf8ByteSpan { start: 40, end: 60 },
            },
            // 同じ文献をもう一度引用しても、番号は初出のものを使う。
            NoteCitationQuery {
                citation_index: 2,
                keys: vec!["smith2024".into()],
                locator: None,
                span: Utf8ByteSpan { start: 70, end: 90 },
            },
        ];

        let resolved = application
            .citation_resolutions(&OneItemLibrary::owner(), &queries, CitationStyle::Numeric)
            .await
            .expect("resolve citations");

        assert_eq!(inline_text(&resolved.resolutions[0]), "[1]");
        assert_eq!(inline_text(&resolved.resolutions[1]), "[2]");
        assert_eq!(inline_text(&resolved.resolutions[2]), "[1]");
        assert_eq!(
            resolved
                .entries
                .iter()
                .map(|entry| entry.citation_key.as_str())
                .collect::<Vec<_>>(),
            vec!["smith2024", "tanaka2025"]
        );
        // 番号は項目の見出しとして持たせる。書誌情報の記述には混ぜない。
        assert_eq!(resolved.entries[0].label.as_deref(), Some("1"));
        assert_eq!(resolved.entries[1].label.as_deref(), Some("2"));
        assert!(resolved.entries[0].text.starts_with("Smith"));
    }

    /// 一つの引用が複数のkeyを名指す場合は、番号を読点で並べる。
    #[tokio::test]
    async fn the_numeric_style_joins_several_keys_in_one_citation() {
        let application = citation_application();
        let queries = vec![NoteCitationQuery {
            citation_index: 0,
            keys: vec!["smith2024".into(), "tanaka2025".into()],
            locator: None,
            span: Utf8ByteSpan { start: 10, end: 40 },
        }];

        let resolved = application
            .citation_resolutions(&OneItemLibrary::owner(), &queries, CitationStyle::Numeric)
            .await
            .expect("resolve citations");

        assert_eq!(inline_text(&resolved.resolutions[0]), "[1, 2]");
    }

    /// 番号で示すスタイルでも、解決できたkeyは一覧の項目へlinkする。
    #[tokio::test]
    async fn the_numeric_style_keeps_the_link_to_the_reference_list() {
        let application = citation_application();
        let queries = vec![NoteCitationQuery {
            citation_index: 0,
            keys: vec!["smith2024".into()],
            locator: None,
            span: Utf8ByteSpan { start: 10, end: 30 },
        }];

        let resolved = application
            .citation_resolutions(&OneItemLibrary::owner(), &queries, CitationStyle::Numeric)
            .await
            .expect("resolve citations");

        let linked = resolved.resolutions[0]
            .segments
            .iter()
            .find(|segment| segment.anchor.is_some())
            .expect("linkする断片");
        assert_eq!(linked.text, "1");
        assert_eq!(linked.anchor.as_deref(), Some("smith2024"));
    }

    /// 引用の表示を、断片をつないだ1つの文字列にする。
    fn inline_text(resolution: &NoteCitationResolution) -> String {
        resolution
            .segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect()
    }

    fn citation_application() -> NoteApplication {
        let repository = Arc::new(MemoryNotes::default());
        NoteApplication::new(
            repository.clone(),
            repository.clone(),
            repository.clone(),
            Arc::new(AcceptContent::default()),
            Arc::new(OneItemLibrary),
            Arc::new(NoLinks),
            Arc::new(FixedClock),
            Arc::new(FixedRandom),
        )
    }

    /// 引用は指定した所有者のライブラリーで解決し、未登録のkeyは警告として報告する。
    #[tokio::test]
    async fn citations_resolve_for_the_named_owner_and_report_unknown_keys() {
        let repository = Arc::new(MemoryNotes::default());
        let application = NoteApplication::new(
            repository.clone(),
            repository.clone(),
            repository.clone(),
            Arc::new(AcceptContent::default()),
            Arc::new(OneItemLibrary),
            Arc::new(NoLinks),
            Arc::new(FixedClock),
            Arc::new(FixedRandom),
        );
        let queries = vec![
            NoteCitationQuery {
                citation_index: 0,
                keys: vec!["smith2024".into(), "missing2024".into()],
                locator: Some("p. 12".into()),
                span: Utf8ByteSpan { start: 10, end: 40 },
            },
            NoteCitationQuery {
                citation_index: 1,
                keys: vec!["smith2024".into()],
                locator: None,
                span: Utf8ByteSpan { start: 60, end: 80 },
            },
        ];

        let resolved = application
            .citation_resolutions(&OneItemLibrary::owner(), &queries, CitationStyle::default())
            .await
            .expect("resolve citations");

        assert_eq!(
            resolved.resolutions[0].segments,
            vec![
                NoteCitationSegment {
                    text: "(".into(),
                    anchor: None,
                },
                NoteCitationSegment {
                    text: "Smith 2024".into(),
                    anchor: Some("smith2024".into()),
                },
                NoteCitationSegment {
                    text: "; ".into(),
                    anchor: None,
                },
                NoteCitationSegment {
                    text: "missing2024".into(),
                    anchor: None,
                },
                NoteCitationSegment {
                    text: ", p. 12)".into(),
                    anchor: None,
                },
            ]
        );
        // 同じ文献を2回引用しても、参考文献一覧の項目は1つになる。
        assert_eq!(
            resolved.entries,
            vec![NoteBibliographyEntry {
                citation_key: "smith2024".into(),
                text: "Smith, A. (2024). An Example Article.".into(),
                label: None,
            }]
        );
        assert_eq!(resolved.diagnostics.len(), 1);
        assert_eq!(resolved.diagnostics[0].code, "unknown_citation_key");
        assert_eq!(
            resolved.diagnostics[0].severity,
            NoteAdvisorySeverity::Warning
        );
        assert_eq!(
            resolved.diagnostics[0].span,
            Some(Utf8ByteSpan { start: 10, end: 40 })
        );

        // 別の利用者のライブラリーでは解決せず、生の識別子だけが並ぶ。
        let other = Identity::new("https://id.example.test".into(), "bob".into()).expect("owner");
        let resolved = application
            .citation_resolutions(&other, &queries, CitationStyle::default())
            .await
            .expect("resolve citations for another owner");
        assert!(resolved.entries.is_empty());
        assert_eq!(resolved.diagnostics.len(), 2);
    }

    #[tokio::test]
    async fn creates_a_note_using_only_application_ports() {
        let repository = Arc::new(MemoryNotes::default());
        let application = NoteApplication::new(
            repository.clone(),
            repository.clone(),
            repository.clone(),
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoLinks),
            Arc::new(FixedClock),
            Arc::new(FixedRandom),
        );
        let actor =
            Actor::try_new("https://id.example.test".into(), "alice".into()).expect("valid actor");

        let created = application
            .create_note(
                actor.clone(),
                NoteDraft {
                    source: "= Portで作成\n:marginalis-tags: 設計\n\n本文".into(),
                    title: "Portで作成".into(),
                    tags: vec!["設計".into()],
                },
                NoteWritePolicy::AllowAdvisories,
            )
            .await
            .expect("create note");

        assert_eq!(created.creator_subject(), "alice");
        assert_eq!(created.revision().get(), 1);
        assert_eq!(
            application
                .read_note(actor, created.note_id())
                .await
                .expect("read created note"),
            created
        );
        assert_eq!(repository.notes.lock().expect("notes lock").len(), 1);
    }

    #[tokio::test]
    async fn preview_preserves_advisories_without_reanalyzing_or_blocking_save() {
        let repository = Arc::new(MemoryNotes::default());
        let content = Arc::new(AcceptContent::default());
        let application = NoteApplication::new(
            repository.clone(),
            repository.clone(),
            repository.clone(),
            content.clone(),
            Arc::new(EmptyLibrary),
            Arc::new(NoLinks),
            Arc::new(FixedClock),
            Arc::new(FixedRandom),
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
            .create_note(actor, draft, NoteWritePolicy::AllowAdvisories)
            .await
            .expect("warning does not reject save");
        assert_eq!(repository.notes.lock().expect("notes lock").len(), 1);
    }

    #[tokio::test]
    async fn strict_writes_reject_warnings_before_mutating_the_repository() {
        let repository = Arc::new(MemoryNotes::default());
        let application = NoteApplication::new(
            repository.clone(),
            repository.clone(),
            repository.clone(),
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoLinks),
            Arc::new(FixedClock),
            Arc::new(FixedRandom),
        );
        let actor =
            Actor::try_new("https://id.example.test".into(), "alice".into()).expect("valid actor");
        let draft = NoteDraft {
            source: "= Warning\n\nbody".into(),
            title: "Warning".into(),
            tags: Vec::new(),
        };

        let create_error = application
            .create_note(
                actor.clone(),
                draft.clone(),
                NoteWritePolicy::RejectWarnings,
            )
            .await
            .expect_err("warning must reject strict create");
        let NoteUseCaseError::AdvisoriesRejected(diagnostics) = create_error else {
            panic!("strict create returned a different error");
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, NoteAdvisorySeverity::Warning);
        assert!(repository.notes.lock().expect("notes lock").is_empty());

        let existing = application
            .create_note(
                actor.clone(),
                draft.clone(),
                NoteWritePolicy::AllowAdvisories,
            )
            .await
            .expect("REST-compatible write accepts warnings");
        let update_error = application
            .update_note(
                actor.clone(),
                existing.note_id(),
                draft,
                existing.revision(),
                NoteWritePolicy::RejectWarnings,
            )
            .await
            .expect_err("warning must reject strict update");
        assert!(matches!(
            update_error,
            NoteUseCaseError::AdvisoriesRejected(_)
        ));
        assert_eq!(repository.update_calls.load(Ordering::Relaxed), 0);
        let unchanged = application
            .read_note(actor, existing.note_id())
            .await
            .expect("stored note remains readable");
        assert_eq!(unchanged.source(), existing.source());
        assert_eq!(unchanged.revision(), Revision::INITIAL);
    }

    #[test]
    fn strict_write_policy_allows_information_and_hints_without_a_warning() {
        let diagnostics = [
            NoteAdvisorySeverity::Information,
            NoteAdvisorySeverity::Hint,
        ]
        .into_iter()
        .map(|severity| NoteAdvisoryDiagnostic {
            code: "test-advisory".into(),
            severity,
            target: NoteValidationTarget::Source,
            span: None,
            message: "test advisory".into(),
        })
        .collect();

        assert_eq!(
            reject_warnings(NoteWritePolicy::RejectWarnings, diagnostics),
            Ok(())
        );
    }

    fn graph_note_id(last: u32) -> NoteId {
        NoteId::new(
            EntityId::from_str(&format!("0197c9bc-0000-7000-8000-{last:012x}")).expect("note ID"),
        )
    }

    /// 鎖状の4件のノートと、末尾のノートが引用する文献1件からなる図を作る。
    fn chain_graph() -> NoteGraph {
        NoteGraph {
            notes: (1..=4)
                .map(|index| NoteGraphNote {
                    note_id: graph_note_id(index),
                    title: format!("ノート{index}"),
                    tags: Vec::new(),
                    updated_at: UnixMillis::new(index.into()),
                })
                .collect(),
            works: vec![NoteGraphWork {
                citation_key: "smith2024".into(),
                title: None,
            }],
            references: (1..4)
                .map(|index| NoteGraphReference {
                    source_note_id: graph_note_id(index),
                    target_note_id: graph_note_id(index + 1),
                })
                .collect(),
            citations: vec![NoteGraphCitation {
                source_note_id: graph_note_id(4),
                citation_key: "smith2024".into(),
            }],
        }
    }

    #[test]
    fn the_graph_keeps_only_what_is_reachable_from_the_origin() {
        let within_one = chain_graph().within(graph_note_id(1), 1);
        assert_eq!(
            within_one
                .notes
                .iter()
                .map(|note| note.title.as_str())
                .collect::<Vec<_>>(),
            ["ノート1", "ノート2"]
        );
        // 両端が残る線だけを返す。ノート2から先の線は残らない。
        assert_eq!(within_one.references.len(), 1);
        assert!(within_one.works.is_empty());

        // 線を3本辿ると末尾のノートまで届く。文献はその1本先にあるため、まだ現れない。
        let within_three = chain_graph().within(graph_note_id(1), 3);
        assert_eq!(within_three.notes.len(), 4);
        assert!(within_three.works.is_empty());
        assert!(within_three.citations.is_empty());

        // もう1本辿ると、末尾のノートが引用している文献も点として現れる。
        let within_four = chain_graph().within(graph_note_id(1), 4);
        assert_eq!(within_four.works.len(), 1);
        assert_eq!(within_four.citations.len(), 1);

        // 向きを問わずに辿る。末尾から始めても先頭へ届く。
        assert_eq!(chain_graph().within(graph_note_id(4), 3).notes.len(), 4);

        // 上限を超える指定は上限として扱う。
        assert_eq!(
            chain_graph().within(graph_note_id(1), MAX_GRAPH_DEPTH + 10),
            chain_graph().within(graph_note_id(1), MAX_GRAPH_DEPTH)
        );
    }

    #[test]
    fn the_graph_is_empty_when_the_origin_is_not_visible() {
        assert_eq!(
            chain_graph().within(graph_note_id(99), 3),
            NoteGraph::default()
        );
    }
}
