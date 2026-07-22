//! HTTP、SQLite、ファイルシステムから独立したユースケースとport。

use marginalis_domain::{
    EntityId, NoteId, NoteProjection, OidcIdentity, OidcLoginResult, RegistrationPolicy,
    SourceRevision, UnixMillis, UserId,
};
use std::future::Future;

/// 時刻取得を外部化し、期限・journal復旧を決定的に試験できるようにする。
pub trait Clock: Send + Sync {
    fn now(&self) -> UnixMillis;
}

/// UUIDv7と秘密トークンの生成を外部化する。
///
/// 実装は暗号学的に安全な乱数を使う。テスト実装は決定的な値を供給できる。
pub trait Random: Send + Sync {
    fn uuid_v7(&self) -> EntityId;
    fn opaque_token(&self) -> String;
}

/// OIDC identityと内部ユーザーの原子的な対応付けを担うport。
pub trait OidcIdentityStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn register_or_lookup(
        &self,
        identity: OidcIdentity,
        policy: RegistrationPolicy,
        new_user_id: UserId,
        now: UnixMillis,
    ) -> impl Future<Output = Result<OidcLoginResult, Self::Error>> + Send;
}

/// OIDC callback adapterが呼ぶ登録ユースケース。
pub struct OidcRegistrationService<'a, Store, Entropy> {
    store: &'a Store,
    entropy: &'a Entropy,
}

impl<'a, Store, Entropy> OidcRegistrationService<'a, Store, Entropy>
where
    Store: OidcIdentityStore,
    Entropy: Random,
{
    pub const fn new(store: &'a Store, entropy: &'a Entropy) -> Self {
        Self { store, entropy }
    }

    pub fn register_or_lookup(
        &self,
        identity: OidcIdentity,
        policy: RegistrationPolicy,
        now: UnixMillis,
    ) -> impl Future<Output = Result<OidcLoginResult, Store::Error>> + Send + '_ {
        self.store
            .register_or_lookup(identity, policy, UserId::new(self.entropy.uuid_v7()), now)
    }
}

/// 一連のファイル・投影更新を復旧可能にする操作ジャーナルの識別子。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationId(pub EntityId);

/// application層が扱う、ファイル正本の更新状態。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationState {
    Prepared,
    SourceApplied,
    Completed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteOperationKind {
    Create,
    Update,
    Delete,
}

/// SQLiteとファイルをまたぐノート更新の復旧に必要な最小情報。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalEntry {
    pub operation_id: OperationId,
    pub note_id: NoteId,
    pub kind: NoteOperationKind,
    pub state: OperationState,
    pub source_revision: Option<SourceRevision>,
    pub projection: Option<NoteProjection>,
    pub created_at: UnixMillis,
    pub updated_at: UnixMillis,
}

/// adapterが実装する操作ジャーナル境界。
pub trait OperationJournal: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn prepare(&self, entry: JournalEntry) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn mark_source_applied(
        &self,
        operation_id: OperationId,
        updated_at: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn complete(
        &self,
        operation_id: OperationId,
        updated_at: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn incomplete(&self) -> impl Future<Output = Result<Vec<JournalEntry>, Self::Error>> + Send;
}

/// AsciiDoc正本を扱うport。HTTP・SQLite・filesystem adapterはこれを実装する。
pub trait NoteSourceStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn read(
        &self,
        note_id: NoteId,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send;

    fn replace(
        &self,
        note_id: NoteId,
        operation: OperationId,
        source: Vec<u8>,
    ) -> impl Future<Output = Result<SourceRevision, Self::Error>> + Send;
}

/// SQLiteなどの検索用投影を、正本更新後に置換するport。
pub trait NoteProjectionStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn replace_projection(
        &self,
        projection: NoteProjection,
        revision: SourceRevision,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// ファイル正本、SQLite投影、操作journalを一貫して更新するユースケース。
pub struct NoteWriteService<'a, Sources, Projections, Journal, Entropy, Time> {
    sources: &'a Sources,
    projections: &'a Projections,
    journal: &'a Journal,
    entropy: &'a Entropy,
    clock: &'a Time,
}

#[derive(Debug)]
pub enum NoteWriteError {
    Journal(Box<dyn std::error::Error + Send + Sync>),
    Source(Box<dyn std::error::Error + Send + Sync>),
    Projection(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for NoteWriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Journal(_) => formatter.write_str("note operation journal failed"),
            Self::Source(_) => formatter.write_str("note source update failed"),
            Self::Projection(_) => formatter.write_str("note projection update failed"),
        }
    }
}

impl std::error::Error for NoteWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Journal(error) | Self::Source(error) | Self::Projection(error) => {
                Some(error.as_ref())
            }
        }
    }
}

impl<'a, Sources, Projections, Journal, Entropy, Time>
    NoteWriteService<'a, Sources, Projections, Journal, Entropy, Time>
where
    Sources: NoteSourceStore,
    Projections: NoteProjectionStore,
    Journal: OperationJournal,
    Entropy: Random,
    Time: Clock,
{
    pub const fn new(
        sources: &'a Sources,
        projections: &'a Projections,
        journal: &'a Journal,
        entropy: &'a Entropy,
        clock: &'a Time,
    ) -> Self {
        Self {
            sources,
            projections,
            journal,
            entropy,
            clock,
        }
    }

    /// sourceは先にfsyncされ、投影失敗時にはjournalを残して起動時復旧の対象にする。
    pub async fn replace(
        &self,
        kind: NoteOperationKind,
        projection: NoteProjection,
        source: Vec<u8>,
    ) -> Result<SourceRevision, NoteWriteError> {
        let operation_id = OperationId(self.entropy.uuid_v7());
        let now = self.clock.now();
        let expected_revision = SourceRevision::from_source(&source);
        self.journal
            .prepare(JournalEntry {
                operation_id,
                note_id: projection.note_id,
                kind,
                state: OperationState::Prepared,
                source_revision: Some(expected_revision),
                projection: Some(projection.clone()),
                created_at: now,
                updated_at: now,
            })
            .await
            .map_err(|error| NoteWriteError::Journal(Box::new(error)))?;
        let revision = self
            .sources
            .replace(projection.note_id, operation_id, source)
            .await
            .map_err(|error| NoteWriteError::Source(Box::new(error)))?;
        self.journal
            .mark_source_applied(operation_id, self.clock.now())
            .await
            .map_err(|error| NoteWriteError::Journal(Box::new(error)))?;
        self.projections
            .replace_projection(projection, revision)
            .await
            .map_err(|error| NoteWriteError::Projection(Box::new(error)))?;
        self.journal
            .complete(operation_id, self.clock.now())
            .await
            .map_err(|error| NoteWriteError::Journal(Box::new(error)))?;
        Ok(revision)
    }

    /// sourceを書込み済みで止まった操作だけを再投影する。preparedは正本変更前なので残す。
    pub async fn recover(&self) -> Result<(), NoteWriteError> {
        for entry in self
            .journal
            .incomplete()
            .await
            .map_err(|error| NoteWriteError::Journal(Box::new(error)))?
        {
            if entry.state != OperationState::SourceApplied {
                continue;
            }
            let Some(projection) = entry.projection else {
                continue;
            };
            let Some(source) = self
                .sources
                .read(entry.note_id)
                .await
                .map_err(|error| NoteWriteError::Source(Box::new(error)))?
            else {
                continue;
            };
            let revision = SourceRevision::from_source(&source);
            if entry.source_revision != Some(revision) {
                continue;
            }
            self.projections
                .replace_projection(projection, revision)
                .await
                .map_err(|error| NoteWriteError::Projection(Box::new(error)))?;
            self.journal
                .complete(entry.operation_id, self.clock.now())
                .await
                .map_err(|error| NoteWriteError::Journal(Box::new(error)))?;
        }
        Ok(())
    }
}
