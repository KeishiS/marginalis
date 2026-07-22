//! AsciiDoc正本をdata directory内で原子的に扱うfilesystem adapter。

use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use marginalis_application::{NoteSourceStore, OperationId};
use marginalis_domain::{NoteId, SourceRevision};

#[derive(Debug)]
pub enum FileStoreError {
    Io(io::Error),
}

impl NoteSourceStore for FileNoteStore {
    type Error = FileStoreError;

    fn read(
        &self,
        note_id: NoteId,
    ) -> impl std::future::Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send {
        std::future::ready(FileNoteStore::read(self, note_id))
    }

    fn replace(
        &self,
        note_id: NoteId,
        operation: OperationId,
        source: Vec<u8>,
    ) -> impl std::future::Future<Output = Result<SourceRevision, Self::Error>> + Send {
        std::future::ready(FileNoteStore::replace(self, note_id, operation, &source))
    }

    fn delete(
        &self,
        note_id: NoteId,
        operation: OperationId,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        std::future::ready(FileNoteStore::delete(self, note_id, operation))
    }
}

impl fmt::Display for FileStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "note source file operation failed: {error}"),
        }
    }
}

impl std::error::Error for FileStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
        }
    }
}

impl From<io::Error> for FileStoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// `dataDir/notes/<note UUID>.adoc`だけを正本として扱う。
#[derive(Clone, Debug)]
pub struct FileNoteStore {
    notes_directory: PathBuf,
}

impl FileNoteStore {
    pub fn open(data_directory: impl AsRef<Path>) -> Result<Self, FileStoreError> {
        let notes_directory = data_directory.as_ref().join("notes");
        fs::create_dir_all(&notes_directory)?;
        Ok(Self { notes_directory })
    }

    /// 指定ノートの正本を返す。未作成ノートは`None`であり、パス情報は公開しない。
    pub fn read(&self, note_id: NoteId) -> Result<Option<Vec<u8>>, FileStoreError> {
        match fs::read(self.note_path(note_id)) {
            Ok(source) => Ok(Some(source)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// 正本をtemp fileへ完全に書き出して同期してからrenameする。
    ///
    /// temp file名はapplication層の操作IDから作るため、任意パスや隠れた乱数生成を持ち込まない。
    /// 同じ操作IDを再利用する呼出しは失敗し、journal復旧処理が明示的に判断する。
    pub fn replace(
        &self,
        note_id: NoteId,
        operation_id: OperationId,
        source: &[u8],
    ) -> Result<SourceRevision, FileStoreError> {
        let target = self.note_path(note_id);
        let temporary = self.temporary_path(note_id, operation_id);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(source)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, target)?;
        self.sync_notes_directory()?;
        Ok(SourceRevision::from_source(source))
    }

    /// 正本を物理削除し、directory entryを同期する。既にない場合も復旧時に成功として扱う。
    pub fn delete(
        &self,
        note_id: NoteId,
        _operation_id: OperationId,
    ) -> Result<(), FileStoreError> {
        match fs::remove_file(self.note_path(note_id)) {
            Ok(()) => {
                self.sync_notes_directory()?;
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// 復旧処理だけが呼ぶ、未完了操作のtemp file除去。
    pub fn discard_temporary(
        &self,
        note_id: NoteId,
        operation_id: OperationId,
    ) -> Result<(), FileStoreError> {
        match fs::remove_file(self.temporary_path(note_id, operation_id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn note_path(&self, note_id: NoteId) -> PathBuf {
        self.notes_directory.join(format!("{note_id}.adoc"))
    }

    fn temporary_path(&self, note_id: NoteId, operation_id: OperationId) -> PathBuf {
        self.notes_directory
            .join(format!(".{note_id}.{}.tmp", operation_id.0))
    }

    fn sync_notes_directory(&self) -> Result<(), FileStoreError> {
        File::open(&self.notes_directory)?.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use marginalis_application::{
        Clock, JournalEntry, NoteOperationKind, NoteWriteService, OperationId, OperationJournal,
        OperationState, Random,
    };
    use marginalis_domain::{EntityId, NoteProjection, UnixMillis, UserId};
    use marginalis_sqlite::SqliteDatabase;
    use sqlx::Row;
    use uuid::Uuid;

    use super::*;

    fn test_directory() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("marginalis-files-{suffix}"))
    }

    fn note(value: u128) -> NoteId {
        NoteId::new(EntityId::from_uuid_v7(Uuid::from_u128(value | (7 << 76))))
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> UnixMillis {
            UnixMillis::new(100)
        }
    }

    struct FixedRandom;

    impl Random for FixedRandom {
        fn uuid_v7(&self) -> EntityId {
            note(3).entity_id()
        }

        fn opaque_token(&self) -> String {
            "unused".into()
        }
    }

    #[test]
    fn atomically_replaces_only_the_note_path_for_typed_ids() {
        let directory = test_directory();
        let store = FileNoteStore::open(&directory).expect("open file store");
        let note_id = note(1);
        let operation = OperationId(note(2).entity_id());
        let revision = store
            .replace(note_id, operation, b"= first\n")
            .expect("write source");
        assert_eq!(
            store.read(note_id).expect("read source"),
            Some(b"= first\n".to_vec())
        );
        assert_eq!(revision, SourceRevision::from_source(b"= first\n"));
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn physically_deletes_the_note_path_idempotently() {
        let directory = test_directory();
        let store = FileNoteStore::open(&directory).expect("open file store");
        let note_id = note(1);
        store
            .replace(note_id, OperationId(note(2).entity_id()), b"= first\n")
            .expect("write source");
        store
            .delete(note_id, OperationId(note(3).entity_id()))
            .expect("delete source");
        assert_eq!(store.read(note_id).expect("read source"), None);
        store
            .delete(note_id, OperationId(note(3).entity_id()))
            .expect("idempotent delete");
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn write_service_updates_source_projection_and_journal() {
        let directory = test_directory();
        let sources = FileNoteStore::open(&directory).expect("open file store");
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("open database");
        let note_id = note(1);
        let owner_id = UserId::new(note(2).entity_id());
        sqlx::query(
            "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
             VALUES (?, 'oidc', 'active', 'Owner', 0, 0)",
        )
        .bind(owner_id.to_string())
        .execute(database.pool())
        .await
        .expect("insert owner");
        let projection = NoteProjection {
            note_id,
            owner_id,
            title: "First note".into(),
            anchors: vec!["start".into()],
            references: Vec::new(),
        };
        let journal = database.operation_journal();
        let projections = database.note_projection_store();
        let service =
            NoteWriteService::new(&sources, &projections, &journal, &FixedRandom, &FixedClock);
        service
            .replace(
                NoteOperationKind::Create,
                projection,
                b"= First note\n".to_vec(),
            )
            .await
            .expect("write note");
        assert_eq!(
            sources.read(note_id).expect("read source"),
            Some(b"= First note\n".to_vec())
        );
        let title: String = sqlx::query("SELECT title FROM notes WHERE note_id = ?")
            .bind(note_id.to_string())
            .fetch_one(database.pool())
            .await
            .expect("read projection")
            .try_get("title")
            .expect("title");
        assert_eq!(title, "First note");
        let incomplete: i64 = sqlx::query(
            "SELECT COUNT(*) AS count FROM operation_journal WHERE state <> 'completed'",
        )
        .fetch_one(database.pool())
        .await
        .expect("read journal")
        .try_get("count")
        .expect("count");
        assert_eq!(incomplete, 0);
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn recovery_replays_a_source_applied_operation() {
        let directory = test_directory();
        let sources = FileNoteStore::open(&directory).expect("open file store");
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let note_id = note(11);
        let owner_id = UserId::new(note(12).entity_id());
        sqlx::query(
            "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
             VALUES (?, 'oidc', 'active', 'Owner', 0, 0)",
        ).bind(owner_id.to_string()).execute(database.pool()).await.expect("owner");
        let projection = NoteProjection {
            note_id,
            owner_id,
            title: "Recovered".into(),
            anchors: Vec::new(),
            references: Vec::new(),
        };
        let operation = OperationId(note(13).entity_id());
        let source = b"= Recovered\n".to_vec();
        let revision = SourceRevision::from_source(&source);
        let journal = database.operation_journal();
        journal
            .prepare(JournalEntry {
                operation_id: operation,
                note_id,
                kind: NoteOperationKind::Create,
                state: OperationState::Prepared,
                source_revision: Some(revision),
                projection: Some(projection),
                created_at: UnixMillis::new(1),
                updated_at: UnixMillis::new(1),
            })
            .await
            .expect("prepare");
        sources
            .replace(note_id, operation, &source)
            .expect("write source");
        journal
            .mark_source_applied(operation, UnixMillis::new(2))
            .await
            .expect("mark source");
        let projections = database.note_projection_store();
        NoteWriteService::new(&sources, &projections, &journal, &FixedRandom, &FixedClock)
            .recover()
            .await
            .expect("recover");
        let title: String = sqlx::query("SELECT title FROM notes WHERE note_id = ?")
            .bind(note_id.to_string())
            .fetch_one(database.pool())
            .await
            .expect("projection")
            .try_get("title")
            .expect("title");
        assert_eq!(title, "Recovered");
        assert!(journal.incomplete().await.expect("journal").is_empty());
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
