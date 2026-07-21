//! SQLite上のアプリ固有投影と参照解決を扱う永続化境界。

use core::fmt;
use std::{sync::Arc, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use tokio::sync::Mutex;

/// ノート参照を解決する利用者の認可済み文脈。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Viewer {
    pub user_id: String,
    pub is_root: bool,
}

/// Base URLから得たアプリ内ノートURLの生成規則。
///
/// ここで保持するのは公開URLのpath部分だけである。スキームとホストはHTTP層が扱い、
/// HTMLには同一オリジンの絶対パスを渡す。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteUrlBase(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidNoteUrlBase;

impl fmt::Display for InvalidNoteUrlBase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Base URL path must start with '/' and contain no query or fragment")
    }
}

impl std::error::Error for InvalidNoteUrlBase {}

impl NoteUrlBase {
    pub fn new(path: impl Into<String>) -> Result<Self, InvalidNoteUrlBase> {
        let mut path = path.into();
        if !path.starts_with('/') || path.contains('?') || path.contains('#') {
            return Err(InvalidNoteUrlBase);
        }
        while path.len() > 1 && path.ends_with('/') {
            path.pop();
        }
        Ok(Self(path))
    }

    pub fn note_href(&self, note_id: &str, anchor: Option<&str>) -> String {
        let prefix = if self.0 == "/" { "" } else { &self.0 };
        let mut href = format!("{prefix}/notes/{note_id}");
        if let Some(anchor) = anchor {
            href.push('#');
            href.push_str(anchor);
        }
        href
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
    Resolved { href: String },
    /// 参照先は閲覧できるがアンカーがないため、ノート先頭へフォールバックした。
    AnchorFallback { href: String },
    /// 対象不在と権限なしを区別せず、対象の存在を秘匿する。
    NotFound,
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
                deleted_at TEXT
            ) STRICT",
        )
        .execute(&self.pool)
        .await?;
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
            "SELECT note_id FROM notes
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
        if accessible.is_none() {
            return Ok(NoteReferenceResolution::NotFound);
        }
        let Some(anchor) = anchor else {
            return Ok(NoteReferenceResolution::Resolved {
                href: urls.note_href(note_id, None),
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
            })
        } else {
            Ok(NoteReferenceResolution::AnchorFallback {
                href: urls.note_href(note_id, None),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use sqlx::Row;

    use super::{NoteReferenceResolution, NoteUrlBase, NotebookStore, StoredNoteReference, Viewer};

    async fn insert_note(store: &NotebookStore, note_id: &str) {
        sqlx::query("INSERT INTO notes(note_id) VALUES (?)")
            .bind(note_id)
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
        let urls = NoteUrlBase::new("/app/").expect("valid base path");

        assert_eq!(
            store
                .resolve_note_reference(&viewer, &urls, "private", None)
                .await
                .expect("resolve private"),
            NoteReferenceResolution::NotFound
        );
        assert_eq!(
            store
                .resolve_note_reference(&viewer, &urls, "visible", Some("missing"))
                .await
                .expect("resolve missing anchor"),
            NoteReferenceResolution::AnchorFallback {
                href: "/app/notes/visible".into()
            }
        );
        assert_eq!(
            store
                .resolve_note_reference(&viewer, &urls, "visible", Some("known"))
                .await
                .expect("resolve known anchor"),
            NoteReferenceResolution::Resolved {
                href: "/app/notes/visible#known".into()
            }
        );
        assert_eq!(
            NoteUrlBase::new("/")
                .expect("valid root base path")
                .note_href("visible", None),
            "/notes/visible"
        );
    }
}
