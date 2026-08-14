//! ノート一覧と単一ノートの問い合わせ。

use marginalis_domain::{
    Actor, DeletedNoteListEntry, NOTE_TEMPLATE_TAG, Note, NoteId, NoteListEntry, Revision,
};

use crate::{NoteListQuery, NoteUseCaseError};

use super::{NoteApplication, NoteOutline};

impl NoteApplication {
    pub async fn list_visible_notes(
        &self,
        actor: Actor,
        query: NoteListQuery,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        self.queries
            .list_visible_notes(&actor, &query)
            .await
            .map_err(NoteUseCaseError::from)
    }

    /// テンプレートノートの一覧。
    ///
    /// [`NOTE_TEMPLATE_TAG`]の付いた閲覧できるノートを、通常の一覧と同じ可視性で返す。
    /// テンプレートは通常のノートであり、専用の保存領域を持たない。
    pub async fn list_note_templates(
        &self,
        actor: Actor,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        Ok(self
            .list_visible_notes(actor, crate::NoteListQuery::default())
            .await?
            .into_iter()
            .filter(|entry| {
                entry
                    .summary
                    .tags
                    .iter()
                    .any(|tag| tag == NOTE_TEMPLATE_TAG)
            })
            .collect())
    }

    pub async fn list_owned_deleted_notes(
        &self,
        actor: Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, NoteUseCaseError> {
        self.queries
            .list_owned_deleted_notes(&actor)
            .await
            .map_err(NoteUseCaseError::from)
    }

    pub async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        self.read_visible_note(&actor, note_id).await
    }

    /// 本文を返さず、見出しの階層と行範囲を返す。
    ///
    /// 保存済みの原文を解析できない場合は、破損として一時障害と区別する。
    pub async fn read_note_outline(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<(Note, NoteOutline), NoteUseCaseError> {
        let note = self.read_visible_note(&actor, note_id).await?;
        let outline = self
            .content
            .outline(note.source())
            .map_err(|_| NoteUseCaseError::CorruptData)?;
        Ok((note, outline))
    }

    /// 指定した行範囲(両端を含む1始まり)の原文断片を返す。
    ///
    /// 断片は原文のbyte列をそのまま切り出す。範囲の末尾が原文の途中なら改行で終わり、
    /// 最終行を含む場合だけ原文の末尾改行の有無に従う。`expected_revision`を指定した
    /// 場合は、行範囲の根拠にした版と現在の版の食い違いを、本文を返す前に競合として拒否する。
    pub async fn read_note_fragment(
        &self,
        actor: Actor,
        note_id: NoteId,
        start_line: usize,
        end_line: usize,
        expected_revision: Option<Revision>,
    ) -> Result<(Note, String), NoteUseCaseError> {
        let note = self.read_visible_note(&actor, note_id).await?;
        if expected_revision.is_some_and(|expected| note.revision() != expected) {
            return Err(NoteUseCaseError::Conflict);
        }
        let fragment = source_fragment(note.source(), start_line, end_line)
            .ok_or(NoteUseCaseError::InvalidLineRange)?;
        Ok((note, fragment))
    }
}

/// 1始まりの行範囲を原文から切り出す。範囲が原文に収まらない場合はNone。
fn source_fragment(source: &str, start_line: usize, end_line: usize) -> Option<String> {
    if start_line == 0 || end_line < start_line {
        return None;
    }
    let had_trailing_newline = source.ends_with('\n');
    let lines: Vec<&str> = if source.is_empty() {
        Vec::new()
    } else if had_trailing_newline {
        let mut lines: Vec<&str> = source.split('\n').collect();
        lines.pop();
        lines
    } else {
        source.split('\n').collect()
    };
    if end_line > lines.len() {
        return None;
    }
    let mut fragment = lines[start_line - 1..end_line].join("\n");
    if end_line < lines.len() || had_trailing_newline {
        fragment.push('\n');
    }
    Some(fragment)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use marginalis_domain::{Actor, NOTE_TEMPLATE_TAG, NoteCreationSource, NoteDraft};

    use crate::NoteWritePolicy;
    use crate::notes::test_support::{
        AcceptContent, EmptyLibrary, MemoryNotes, NoMathMacros, note_application,
    };

    use super::source_fragment;

    /// テンプレート一覧は、識別タグの付いたノートだけを返す。
    #[tokio::test]
    async fn template_listing_returns_only_tagged_notes() {
        let repository = Arc::new(MemoryNotes::default());
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let actor =
            Actor::try_new("https://id.example.test".into(), "alice".into()).expect("valid actor");
        for (title, tags) in [
            ("実験記録の雛形", vec![NOTE_TEMPLATE_TAG.to_owned()]),
            ("通常のノート", vec!["研究".to_owned()]),
        ] {
            application
                .create_note(
                    actor.clone(),
                    NoteDraft {
                        source: format!("= {title}\n\n本文"),
                        title: title.into(),
                        tags,
                    },
                    NoteWritePolicy::AllowAdvisories,
                    NoteCreationSource::Rest,
                )
                .await
                .expect("create note");
        }

        let templates = application
            .list_note_templates(actor)
            .await
            .expect("list templates");

        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].summary.title, "実験記録の雛形");
    }

    /// 断片は原文の行をそのまま切り出し、末尾改行は範囲の位置に従う。
    #[test]
    fn fragments_preserve_lines_and_trailing_newlines() {
        let source = "= Title\n\nfirst\nsecond";
        assert_eq!(
            source_fragment(source, 1, 2).as_deref(),
            Some("= Title\n\n")
        );
        assert_eq!(
            source_fragment(source, 3, 4).as_deref(),
            Some("first\nsecond")
        );
        assert_eq!(
            source_fragment("first\nsecond\n", 1, 2).as_deref(),
            Some("first\nsecond\n")
        );
    }

    /// 0行目、逆転した範囲、原文の外の行は拒否する。
    #[test]
    fn rejects_ranges_outside_the_source() {
        let source = "first\nsecond\n";
        assert_eq!(source_fragment(source, 0, 1), None);
        assert_eq!(source_fragment(source, 2, 1), None);
        assert_eq!(source_fragment(source, 1, 3), None);
        assert_eq!(source_fragment("", 1, 1), None);
    }
}
