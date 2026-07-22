//! SQLite上のアプリ固有投影と参照解決を扱う永続化境界。

use core::fmt;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use tokio::sync::Mutex;
use url::Url;

use adocweave::{
    reference::{ResolutionFailureKind, ResolvedReference, ResolverFailure},
    render::RenderInputs,
    source::TextRange,
};
use notebook_adoc::{NoteReferenceError, extract_note_references};

/// ノート参照を解決する利用者の認可済み文脈。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Viewer {
    pub user_id: String,
    pub is_root: bool,
}

/// 絶対HTTPS Base URLから得たアプリ内ノートURLの生成規則。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteUrlBase(Url);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidNoteUrlBase;

impl fmt::Display for InvalidNoteUrlBase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "Base URL must be an absolute HTTPS URL without credentials, query, or fragment",
        )
    }
}

impl std::error::Error for InvalidNoteUrlBase {}

impl NoteUrlBase {
    pub fn new(value: impl AsRef<str>) -> Result<Self, InvalidNoteUrlBase> {
        let mut url = Url::parse(value.as_ref()).map_err(|_| InvalidNoteUrlBase)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(InvalidNoteUrlBase);
        }
        let path = url.path().trim_end_matches('/').to_owned();
        url.set_path(&path);
        Ok(Self(url))
    }

    pub fn note_href(&self, note_id: &str, anchor: Option<&str>) -> String {
        let mut url = self.0.clone();
        let base_path = url.path().trim_end_matches('/').to_owned();
        url.set_path(&format!("{base_path}/note/{note_id}"));
        url.set_fragment(anchor);
        url.into()
    }
}

/// 本文中の一箇所に対応する、永続化する参照投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredNoteReference {
    pub source_start: i64,
    pub source_end: i64,
    pub target_note_id: String,
    pub target_anchor: Option<String>,
}

/// ACLを適用したノート参照の解決結果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoteReferenceResolution {
    /// 参照先とアンカーを閲覧できる。
    Resolved { href: String, title: String },
    /// 参照先は閲覧できるがアンカーがないため、ノート先頭へフォールバックした。
    AnchorFallback { href: String, title: String },
    /// 対象不在と権限なしを区別せず、対象の存在を秘匿する。
    NotFound {
        /// `root`だけが受け取る詳細。通常利用者には常に`None`を返す。
        detail: Option<ReferenceFailureDetail>,
    },
}

/// 権限を持つ運用文脈だけに返す未解決参照の詳細。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceFailureDetail {
    MissingTarget,
}

/// HTML表示へ渡す前の、アプリ固有で位置付きの参照表示情報。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferencePresentation {
    pub range: TextRange,
    /// 空labelの標準xrefに使う、閲覧権限確認済みの解決先タイトル。
    pub display_label: Option<String>,
    /// 解決には成功したが利用者へ伝えるべき警告。
    pub warning: Option<ReferenceWarning>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceWarning {
    pub code: &'static str,
    pub message: String,
}

/// 一つの解析revisionに対応する、描画へ渡す参照解決結果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedNoteReferences {
    pub render_inputs: RenderInputs,
    /// `RenderInputs`の公開契約にはまだ含まれない、アプリ側の表示情報。
    pub presentations: BTreeMap<TextRange, ReferencePresentation>,
}

/// AsciiDoc上の形式検証とSQLite照会を分離する参照解決エラー。
#[derive(Debug)]
pub enum ResolveReferencesError {
    InvalidReferences(Vec<NoteReferenceError>),
    Database(sqlx::Error),
}

impl fmt::Display for ResolveReferencesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidReferences(_) => formatter.write_str("invalid note reference"),
            Self::Database(error) => {
                write!(formatter, "SQLite reference resolution failed: {error}")
            }
        }
    }
}

impl std::error::Error for ResolveReferencesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidReferences(_) => None,
            Self::Database(error) => Some(error),
        }
    }
}

impl From<sqlx::Error> for ResolveReferencesError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// SQLite上のノート投影ストア。
#[derive(Clone, Debug)]
pub struct NotebookStore {
    pool: SqlitePool,
    write_lock: Arc<Mutex<()>>,
}

impl NotebookStore {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let options = database_url
            .parse::<SqliteConnectOptions>()?
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        let store = Self {
            pool,
            write_lock: Arc::new(Mutex::new(())),
        };
        store.migrate().await?;
        Ok(store)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// SQLiteの参照投影に必要な最小スキーマを作成する。
    pub async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS notes (
                note_id TEXT PRIMARY KEY NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                deleted_at TEXT
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        let note_columns = sqlx::query("PRAGMA table_info(notes)")
            .fetch_all(&self.pool)
            .await?;
        let has_title = note_columns.iter().any(|column| {
            column
                .try_get::<String, _>("name")
                .is_ok_and(|name| name == "title")
        });
        if !has_title {
            sqlx::query("ALTER TABLE notes ADD COLUMN title TEXT NOT NULL DEFAULT ''")
                .execute(&self.pool)
                .await?;
        }
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS note_acl (
                note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
                user_id TEXT NOT NULL,
                permission INTEGER NOT NULL CHECK (permission BETWEEN 1 AND 3),
                PRIMARY KEY (note_id, user_id)
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS note_anchors (
                note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
                anchor_id TEXT NOT NULL,
                PRIMARY KEY (note_id, anchor_id)
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS note_references (
                source_note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
                source_start INTEGER NOT NULL CHECK (source_start >= 0),
                source_end INTEGER NOT NULL CHECK (source_end > source_start),
                target_note_id TEXT NOT NULL,
                target_anchor TEXT,
                PRIMARY KEY (source_note_id, source_start, source_end)
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS note_references_target_idx
             ON note_references(target_note_id, target_anchor)",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 同じノートに属する参照位置をトランザクションで全置換する。
    pub async fn replace_references(
        &self,
        source_note_id: &str,
        references: &[StoredNoteReference],
    ) -> Result<(), sqlx::Error> {
        let _write_guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM note_references WHERE source_note_id = ?")
            .bind(source_note_id)
            .execute(&mut *transaction)
            .await?;
        for reference in references {
            sqlx::query(
                "INSERT INTO note_references \
                 (source_note_id, source_start, source_end, target_note_id, target_anchor) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(source_note_id)
            .bind(reference.source_start)
            .bind(reference.source_end)
            .bind(&reference.target_note_id)
            .bind(&reference.target_anchor)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await
    }

    /// 対象不在とACL拒否を同じ結果に畳み込み、存在を推測できないようにする。
    pub async fn resolve_note_reference(
        &self,
        viewer: &Viewer,
        urls: &NoteUrlBase,
        note_id: &str,
        anchor: Option<&str>,
    ) -> Result<NoteReferenceResolution, sqlx::Error> {
        let accessible = sqlx::query(
            "SELECT note_id, title FROM notes
             WHERE note_id = ? AND deleted_at IS NULL
             AND (? = 1 OR EXISTS (
                 SELECT 1 FROM note_acl
                 WHERE note_acl.note_id = notes.note_id
                 AND user_id = ? AND permission >= 1
             ))",
        )
        .bind(note_id)
        .bind(viewer.is_root)
        .bind(&viewer.user_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(accessible) = accessible else {
            return Ok(NoteReferenceResolution::NotFound {
                detail: viewer
                    .is_root
                    .then_some(ReferenceFailureDetail::MissingTarget),
            });
        };
        let title: String = accessible.try_get("title")?;
        let Some(anchor) = anchor else {
            return Ok(NoteReferenceResolution::Resolved {
                href: urls.note_href(note_id, None),
                title,
            });
        };
        let anchor_exists =
            sqlx::query("SELECT anchor_id FROM note_anchors WHERE note_id = ? AND anchor_id = ?")
                .bind(note_id)
                .bind(anchor)
                .fetch_optional(&self.pool)
                .await?
                .is_some();
        if anchor_exists {
            Ok(NoteReferenceResolution::Resolved {
                href: urls.note_href(note_id, Some(anchor)),
                title,
            })
        } else {
            Ok(NoteReferenceResolution::AnchorFallback {
                href: urls.note_href(note_id, None),
                title,
            })
        }
    }

    /// 解析済みAsciiDocの`xref:note:`を同一revisionの描画入力へ変換する。
    pub async fn resolve_render_inputs(
        &self,
        analysis: &adocweave::Analysis,
        viewer: &Viewer,
        urls: &NoteUrlBase,
    ) -> Result<ResolvedNoteReferences, ResolveReferencesError> {
        let references =
            extract_note_references(analysis).map_err(ResolveReferencesError::InvalidReferences)?;
        let mut render_references = Vec::with_capacity(references.len());
        let mut presentations = BTreeMap::new();
        for reference in references {
            match self
                .resolve_note_reference(
                    viewer,
                    urls,
                    &reference.note_id,
                    reference.anchor.as_deref(),
                )
                .await?
            {
                NoteReferenceResolution::Resolved { href, title } => {
                    if reference.label_is_empty {
                        presentations.insert(
                            reference.range,
                            ReferencePresentation {
                                range: reference.range,
                                display_label: Some(title),
                                warning: None,
                            },
                        );
                    }
                    render_references.push(ResolvedReference::resolved(reference.range, href));
                }
                NoteReferenceResolution::AnchorFallback { href, title } => {
                    presentations.insert(
                        reference.range,
                        ReferencePresentation {
                            range: reference.range,
                            display_label: reference.label_is_empty.then_some(title),
                            warning: Some(ReferenceWarning {
                                code: "missing-reference-anchor",
                                message:
                                    "参照先アンカーが見つからないため、ノート先頭を表示します。"
                                        .into(),
                            }),
                        },
                    );
                    render_references.push(ResolvedReference::resolved(reference.range, href));
                }
                NoteReferenceResolution::NotFound { .. } => {
                    render_references.push(ResolvedReference::failed(
                        reference.range,
                        ResolverFailure {
                            kind: ResolutionFailureKind::MissingTarget,
                            message: "note reference unavailable".into(),
                        },
                    ));
                }
            }
        }
        Ok(ResolvedNoteReferences {
            render_inputs: RenderInputs::new(render_references, Vec::new()),
            presentations,
        })
    }
}

#[cfg(test)]
mod tests {
    use adocweave::{
        Engine,
        reference::{ResolutionFailureKind, ResolutionOutcome, ResolverFailure},
    };
    use sqlx::Row;

    use super::{
        NoteReferenceResolution, NoteUrlBase, NotebookStore, ReferenceFailureDetail,
        StoredNoteReference, Viewer,
    };

    async fn insert_note(store: &NotebookStore, note_id: &str) {
        sqlx::query("INSERT INTO notes(note_id, title) VALUES (?, ?)")
            .bind(note_id)
            .bind(format!("title:{note_id}"))
            .execute(store.pool())
            .await
            .expect("insert note");
    }

    #[tokio::test]
    async fn stores_each_reference_position_separately() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        insert_note(&store, "source").await;
        store
            .replace_references(
                "source",
                &[
                    StoredNoteReference {
                        source_start: 3,
                        source_end: 14,
                        target_note_id: "target".into(),
                        target_anchor: None,
                    },
                    StoredNoteReference {
                        source_start: 21,
                        source_end: 37,
                        target_note_id: "target".into(),
                        target_anchor: Some("details".into()),
                    },
                ],
            )
            .await
            .expect("store positions");

        let count: i64 = sqlx::query("SELECT COUNT(*) AS count FROM note_references")
            .fetch_one(store.pool())
            .await
            .expect("count positions")
            .get("count");
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn hides_forbidden_notes_and_falls_back_for_missing_anchors() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        insert_note(&store, "visible").await;
        insert_note(&store, "private").await;
        sqlx::query(
            "INSERT INTO note_acl(note_id, user_id, permission) VALUES ('visible', 'reader', 1)",
        )
        .execute(store.pool())
        .await
        .expect("grant reader");
        sqlx::query("INSERT INTO note_anchors(note_id, anchor_id) VALUES ('visible', 'known')")
            .execute(store.pool())
            .await
            .expect("insert anchor");
        let viewer = Viewer {
            user_id: "reader".into(),
            is_root: false,
        };
        let urls = NoteUrlBase::new("https://notebook.example/app/").expect("valid Base URL");

        assert_eq!(
            store
                .resolve_note_reference(&viewer, &urls, "private", None)
                .await
                .expect("resolve private"),
            NoteReferenceResolution::NotFound { detail: None }
        );
        assert_eq!(
            store
                .resolve_note_reference(&viewer, &urls, "visible", Some("missing"))
                .await
                .expect("resolve missing anchor"),
            NoteReferenceResolution::AnchorFallback {
                href: "https://notebook.example/app/note/visible".into(),
                title: "title:visible".into(),
            }
        );
        assert_eq!(
            store
                .resolve_note_reference(&viewer, &urls, "visible", Some("known"))
                .await
                .expect("resolve known anchor"),
            NoteReferenceResolution::Resolved {
                href: "https://notebook.example/app/note/visible#known".into(),
                title: "title:visible".into(),
            }
        );
        assert_eq!(
            NoteUrlBase::new("https://notebook.example")
                .expect("valid root Base URL")
                .note_href("visible", None),
            "https://notebook.example/note/visible"
        );
        assert!(NoteUrlBase::new("http://notebook.example").is_err());
        assert!(NoteUrlBase::new("https://notebook.example/app?debug=true").is_err());

        let root = Viewer {
            user_id: "root".into(),
            is_root: true,
        };
        assert_eq!(
            store
                .resolve_note_reference(&root, &urls, "missing", None)
                .await
                .expect("resolve missing as root"),
            NoteReferenceResolution::NotFound {
                detail: Some(ReferenceFailureDetail::MissingTarget),
            }
        );
    }

    #[tokio::test]
    async fn creates_render_inputs_without_disclosing_forbidden_targets() {
        let store = NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        let visible = "01800000-0000-7000-8000-000000000001";
        let private = "01800000-0000-7000-8000-000000000002";
        insert_note(&store, visible).await;
        insert_note(&store, private).await;
        sqlx::query("INSERT INTO note_acl(note_id, user_id, permission) VALUES (?, 'reader', 1)")
            .bind(visible)
            .execute(store.pool())
            .await
            .expect("grant reader");
        let analysis = Engine::new(Default::default())
            .analyze(&format!(
                "xref:note:{visible}#missing[] xref:note:{private}[秘匿]\n"
            ))
            .expect("valid AsciiDoc");
        let viewer = Viewer {
            user_id: "reader".into(),
            is_root: false,
        };
        let urls = NoteUrlBase::new("https://notebook.example/app").expect("valid Base URL");

        let result = store
            .resolve_render_inputs(&analysis, &viewer, &urls)
            .await
            .expect("resolve inputs");
        assert_eq!(result.presentations.len(), 1);
        let presentation = result
            .presentations
            .values()
            .next()
            .expect("fallback presentation");
        let expected_title = format!("title:{visible}");
        assert_eq!(
            presentation.display_label.as_deref(),
            Some(expected_title.as_str())
        );
        assert_eq!(
            presentation.warning.as_ref().map(|warning| warning.code),
            Some("missing-reference-anchor")
        );
        assert_eq!(result.render_inputs.references().len(), 2);
        assert_eq!(
            result.render_inputs.references()[0].outcome,
            ResolutionOutcome::Resolved {
                href: format!("https://notebook.example/app/note/{visible}")
            }
        );
        assert_eq!(
            result.render_inputs.references()[1].outcome,
            ResolutionOutcome::Failed(ResolverFailure {
                kind: ResolutionFailureKind::MissingTarget,
                message: "note reference unavailable".into(),
            })
        );
    }
}
