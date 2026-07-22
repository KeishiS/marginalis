//! HTTP、SQLite、ファイルシステムから独立したユースケースとport。

use marginalis_domain::{
    EntityId, NoteId, OidcIdentity, OidcLoginResult, RegistrationPolicy, SourceRevision,
    UnixMillis, UserId,
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JournalEntry {
    pub operation_id: OperationId,
    pub note_id: NoteId,
    pub kind: NoteOperationKind,
    pub state: OperationState,
    pub source_revision: Option<SourceRevision>,
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
