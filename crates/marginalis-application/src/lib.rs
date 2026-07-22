//! HTTP、SQLite、ファイルシステムから独立したユースケースとport。

use marginalis_domain::{EntityId, NoteId, SourceRevision, UnixMillis};
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

/// 一連のファイル・投影更新を復旧可能にする操作ジャーナルの識別子。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationId(pub EntityId);

/// application層が扱う、ファイル正本の更新状態。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationState {
    Prepared,
    Applied,
    Completed,
}

/// adapterが実装する操作ジャーナル境界。
pub trait OperationJournal: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn record(
        &self,
        operation: OperationId,
        state: OperationState,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn incomplete(&self) -> impl Future<Output = Result<Vec<OperationId>, Self::Error>> + Send;
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
