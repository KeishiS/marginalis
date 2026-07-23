//! AsciiDoc正本をdata directory内で原子的に扱うfilesystem adapter。

use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    str::FromStr,
};

use marginalis_application::{NoteSourceStore, OperationId};
use marginalis_domain::{EntityId, NoteId, SourceRevision};

/// Marginalisが管理するdata directoryの破壊的format識別子。
pub const DATA_FORMAT_VERSION: u32 = 1;
const DATA_FORMAT_FILE: &str = "FORMAT";
const DATA_FORMAT_CONTENT: &str = "marginalis-data-format=1\n";

/// 正本・SQLite・運用metadataを収容するdata directoryの入口。
///
/// markerのない非空directoryは旧formatまたは手動配置とみなし、暗黙移行しない。
#[derive(Clone, Debug)]
pub struct StorageLayout {
    data_directory: PathBuf,
}

#[derive(Debug)]
pub enum StorageLayoutError {
    Io(io::Error),
    Incompatible(PathBuf),
}

impl fmt::Display for StorageLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "data directory operation failed: {error}"),
            Self::Incompatible(path) => write!(
                formatter,
                "data directory is not Marginalis data format v{DATA_FORMAT_VERSION}: {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for StorageLayoutError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Incompatible(_) => None,
        }
    }
}

impl From<io::Error> for StorageLayoutError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl StorageLayout {
    /// 空directoryをformat v1として初期化し、既存directoryはmarkerが一致する場合だけ開く。
    pub fn open(data_directory: impl AsRef<Path>) -> Result<Self, StorageLayoutError> {
        let data_directory = data_directory.as_ref().to_path_buf();
        fs::create_dir_all(&data_directory)?;
        let marker = data_directory.join(DATA_FORMAT_FILE);
        match fs::read_to_string(&marker) {
            Ok(content) if content == DATA_FORMAT_CONTENT => Ok(Self { data_directory }),
            Ok(_) => Err(StorageLayoutError::Incompatible(data_directory)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if fs::read_dir(&data_directory)?.next().is_some() {
                    return Err(StorageLayoutError::Incompatible(data_directory));
                }
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(marker)?;
                file.write_all(DATA_FORMAT_CONTENT.as_bytes())?;
                file.sync_all()?;
                drop(file);
                File::open(&data_directory)?.sync_all()?;
                Ok(Self { data_directory })
            }
            Err(error) => Err(error.into()),
        }
    }

    /// backup/restore入力のように、formatを新規作成せず既存markerだけを検証する。
    pub fn validate_existing(data_directory: impl AsRef<Path>) -> Result<Self, StorageLayoutError> {
        let data_directory = data_directory.as_ref().to_path_buf();
        match fs::read_to_string(data_directory.join(DATA_FORMAT_FILE)) {
            Ok(content) if content == DATA_FORMAT_CONTENT => Ok(Self { data_directory }),
            Ok(_) => Err(StorageLayoutError::Incompatible(data_directory)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(StorageLayoutError::Incompatible(data_directory))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn data_directory(&self) -> &Path {
        &self.data_directory
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_directory.join("marginalis.sqlite")
    }

    pub fn copy_format_to(&self, destination: impl AsRef<Path>) -> Result<(), StorageLayoutError> {
        fs::write(
            destination.as_ref().join(DATA_FORMAT_FILE),
            DATA_FORMAT_CONTENT,
        )?;
        File::open(destination.as_ref())?.sync_all()?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum FileStoreError {
    Io(io::Error),
    InvalidNoteFileName(PathBuf),
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

    fn discard_temporary(
        &self,
        note_id: NoteId,
        operation: OperationId,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        std::future::ready(FileNoteStore::discard_temporary(self, note_id, operation))
    }
}

impl fmt::Display for FileStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "note source file operation failed: {error}"),
            Self::InvalidNoteFileName(path) => {
                write!(
                    formatter,
                    "note source path is not a UUIDv7 AsciiDoc file: {}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for FileStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidNoteFileName(_) => None,
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

    /// `notes/`直下の全`.adoc`正本を、安定したnote ID順で返す。
    ///
    /// 再構築はこの戻り値をすべて検証してからSQLiteを書き換える。`.adoc`の名前がUUIDv7でなければ
    /// 外部編集による破損として失敗し、既存projectionを保持する。
    pub fn list_sources(&self) -> Result<Vec<(NoteId, Vec<u8>)>, FileStoreError> {
        let mut entries = fs::read_dir(&self.notes_directory)?
            .map(|entry| entry.map_err(FileStoreError::from))
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        entries
            .into_iter()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "adoc")
            })
            .map(|entry| {
                let path = entry.path();
                let stem = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| FileStoreError::InvalidNoteFileName(path.clone()))?;
                let note_id = EntityId::from_str(stem)
                    .map(NoteId::new)
                    .map_err(|_| FileStoreError::InvalidNoteFileName(path.clone()))?;
                Ok((note_id, fs::read(path)?))
            })
            .collect()
    }

    /// 検証済みの正本をbackup directoryへ複製する。
    ///
    /// 呼出し側はSQLiteのbackupと同じ停止期間に実行する。出力先には`notes/`だけを作り、
    /// 一時ファイルや正本以外のファイルは持ち込まない。
    pub fn copy_sources_to(
        &self,
        backup_directory: impl AsRef<Path>,
    ) -> Result<usize, FileStoreError> {
        let destination = backup_directory.as_ref().join("notes");
        fs::create_dir(&destination)?;
        let sources = self.list_sources()?;
        for (note_id, source) in &sources {
            let target = destination.join(format!("{note_id}.adoc"));
            fs::write(target, source)?;
        }
        File::open(&destination)?.sync_all()?;
        Ok(sources.len())
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
        sync::atomic::{AtomicUsize, Ordering},
    };

    use marginalis_application::{
        Clock, JournalEntry, NoteOperationKind, NoteWriteError, NoteWriteService, OperationId,
        OperationJournal, OperationState, Random,
    };
    use marginalis_domain::{EntityId, NoteProjection, UnixMillis, UserId};
    use marginalis_sqlite::SqliteDatabase;
    use sqlx::Row;
    use uuid::Uuid;

    use super::*;

    fn test_directory() -> PathBuf {
        std::env::temp_dir().join(format!("marginalis-files-{}", Uuid::now_v7()))
    }

    fn note(value: u128) -> NoteId {
        NoteId::new(EntityId::from_uuid_v7(Uuid::from_u128(value | (7 << 76))))
    }

    #[test]
    fn storage_layout_initializes_only_an_empty_directory() {
        let directory = test_directory();

        let layout = StorageLayout::open(&directory).expect("initialize layout");

        assert_eq!(layout.data_directory(), directory);
        assert_eq!(layout.database_path(), directory.join("marginalis.sqlite"));
        assert_eq!(
            fs::read_to_string(directory.join(DATA_FORMAT_FILE)).expect("format marker"),
            DATA_FORMAT_CONTENT
        );
        StorageLayout::validate_existing(&directory).expect("validate initialized layout");
        fs::remove_dir_all(directory).expect("remove directory");
    }

    #[test]
    fn storage_layout_rejects_nonempty_directory_without_marker() {
        let directory = test_directory();
        fs::create_dir_all(&directory).expect("create directory");
        fs::write(directory.join("marginalis.sqlite"), "legacy").expect("write legacy data");

        assert!(matches!(
            StorageLayout::open(&directory),
            Err(StorageLayoutError::Incompatible(path)) if path == directory
        ));
        fs::remove_dir_all(directory).expect("remove directory");
    }

    #[test]
    fn storage_layout_rejects_unknown_marker_version() {
        let directory = test_directory();
        fs::create_dir_all(&directory).expect("create directory");
        fs::write(
            directory.join(DATA_FORMAT_FILE),
            "marginalis-data-format=2\n",
        )
        .expect("write marker");

        assert!(matches!(
            StorageLayout::validate_existing(&directory),
            Err(StorageLayoutError::Incompatible(path)) if path == directory
        ));
        fs::remove_dir_all(directory).expect("remove directory");
    }

    #[test]
    fn lists_only_uuidv7_asciidoc_sources_in_stable_order() {
        let directory = test_directory();
        let sources = FileNoteStore::open(&directory).expect("open sources");
        let first = note(1);
        let second = note(2);
        fs::write(
            directory.join("notes").join(format!("{second}.adoc")),
            "second",
        )
        .expect("write source");
        fs::write(
            directory.join("notes").join(format!("{first}.adoc")),
            "first",
        )
        .expect("write source");
        fs::write(directory.join("notes").join("ignored.txt"), "ignored").expect("write other");

        assert_eq!(
            sources.list_sources().expect("list sources"),
            vec![(first, b"first".to_vec()), (second, b"second".to_vec())]
        );
        fs::remove_dir_all(directory).expect("remove directory");
    }

    #[test]
    fn rejects_non_uuidv7_asciidoc_source_names() {
        let directory = test_directory();
        let sources = FileNoteStore::open(&directory).expect("open sources");
        fs::write(directory.join("notes").join("not-a-note.adoc"), "invalid")
            .expect("write source");
        assert!(matches!(
            sources.list_sources(),
            Err(FileStoreError::InvalidNoteFileName(_))
        ));
        fs::remove_dir_all(directory).expect("remove directory");
    }

    #[test]
    fn copies_only_validated_canonical_sources_to_backup() {
        let directory = test_directory();
        let sources = FileNoteStore::open(&directory).expect("open sources");
        let note_id = note(1);
        fs::write(
            directory.join("notes").join(format!("{note_id}.adoc")),
            "= canonical\n",
        )
        .expect("write source");
        fs::write(directory.join("notes").join("ignored.tmp"), "temporary")
            .expect("write temporary file");
        let backup = directory.join("backup");
        fs::create_dir(&backup).expect("create backup directory");

        assert_eq!(sources.copy_sources_to(&backup).expect("copy sources"), 1);
        assert_eq!(
            fs::read(backup.join("notes").join(format!("{note_id}.adoc"))).expect("read backup"),
            b"= canonical\n"
        );
        assert!(!backup.join("notes").join("ignored.tmp").exists());
        fs::remove_dir_all(directory).expect("remove directory");
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

    struct SequenceRandom;

    impl Random for SequenceRandom {
        fn uuid_v7(&self) -> EntityId {
            static NEXT: AtomicUsize = AtomicUsize::new(100);
            note(NEXT.fetch_add(1, Ordering::Relaxed) as u128).entity_id()
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
            search_text: "First note".into(),
            anchors: vec!["start".into()],
            references: Vec::new(),
        };
        let journal = database.operation_journal();
        let projections = database.note_projection_store();
        let service = NoteWriteService::new(
            &sources,
            &projections,
            &journal,
            &SequenceRandom,
            &FixedClock,
        );
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
    async fn write_service_physically_deletes_source_and_projection() {
        let directory = test_directory();
        let sources = FileNoteStore::open(&directory).expect("open file store");
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let note_id = note(21);
        let owner_id = UserId::new(note(22).entity_id());
        sqlx::query("INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms) VALUES (?, 'oidc', 'active', 'Owner', 0, 0)")
            .bind(owner_id.to_string()).execute(database.pool()).await.expect("owner");
        let journal = database.operation_journal();
        let projections = database.note_projection_store();
        let service = NoteWriteService::new(
            &sources,
            &projections,
            &journal,
            &SequenceRandom,
            &FixedClock,
        );
        service
            .replace(
                NoteOperationKind::Create,
                NoteProjection {
                    note_id,
                    owner_id,
                    title: "Disposable".into(),
                    search_text: "Disposable".into(),
                    anchors: Vec::new(),
                    references: Vec::new(),
                },
                b"= Disposable\n".to_vec(),
            )
            .await
            .expect("create");
        service.delete(note_id).await.expect("delete");
        assert_eq!(sources.read(note_id).expect("source"), None);
        let notes: i64 = sqlx::query("SELECT COUNT(*) AS count FROM notes WHERE note_id = ?")
            .bind(note_id.to_string())
            .fetch_one(database.pool())
            .await
            .expect("notes")
            .try_get("count")
            .expect("count");
        let acl: i64 = sqlx::query("SELECT COUNT(*) AS count FROM note_acl WHERE note_id = ?")
            .bind(note_id.to_string())
            .fetch_one(database.pool())
            .await
            .expect("acl")
            .try_get("count")
            .expect("count");
        assert_eq!((notes, acl), (0, 0));
        assert!(journal.incomplete().await.expect("journal").is_empty());
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn recovery_detects_a_renamed_source_before_journal_marking() {
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
            search_text: "Recovered".into(),
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

    #[tokio::test]
    async fn recovery_stops_on_an_unexpected_canonical_source() {
        let directory = test_directory();
        let sources = FileNoteStore::open(&directory).expect("open file store");
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let note_id = note(14);
        let owner_id = UserId::new(note(15).entity_id());
        sqlx::query(
            "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
             VALUES (?, 'oidc', 'active', 'Owner', 0, 0)",
        )
        .bind(owner_id.to_string())
        .execute(database.pool())
        .await
        .expect("owner");
        let operation = OperationId(note(16).entity_id());
        let expected = b"= Expected\n".to_vec();
        let journal = database.operation_journal();
        journal
            .prepare(JournalEntry {
                operation_id: operation,
                note_id,
                kind: NoteOperationKind::Create,
                state: OperationState::Prepared,
                source_revision: Some(SourceRevision::from_source(&expected)),
                projection: Some(NoteProjection {
                    note_id,
                    owner_id,
                    title: "Expected".into(),
                    search_text: "Expected".into(),
                    anchors: Vec::new(),
                    references: Vec::new(),
                }),
                created_at: UnixMillis::new(1),
                updated_at: UnixMillis::new(1),
            })
            .await
            .expect("prepare");
        sources
            .replace(note_id, operation, b"= Different\n")
            .expect("write unexpected source");
        let projections = database.note_projection_store();
        assert!(matches!(
            NoteWriteService::new(&sources, &projections, &journal, &FixedRandom, &FixedClock)
                .recover()
                .await,
            Err(NoteWriteError::InconsistentRecovery { .. })
        ));
        assert_eq!(journal.incomplete().await.expect("journal").len(), 1);
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
