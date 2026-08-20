//! applicationが外側へ要求する共通の保存失敗、時刻、乱数の境界。

use marginalis_domain::{EntityId, UnixMillis};

/// 永続化方式に依存しない、repository port共通の失敗理由。
///
/// すべての系統のrepositoryがこの型を返し、系統ごとの意味づけと利用者向けの表現は、
/// 各ユースケースのエラー型とtransport側の写像が決める。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum StorageError {
    #[error("stored entity was not found")]
    NotFound,
    #[error("stored state conflicts with the expected revision")]
    Conflict,
    #[error("restoration period has expired")]
    RetentionExpired,
    /// 保存済みの内容が現行の規則を満たさない。再試行では解消しない。
    #[error("stored data is invalid")]
    CorruptData,
    /// 一時的に処理できない。再試行で解消しうる。
    #[error("storage is unavailable")]
    Unavailable,
}

pub trait Clock: Send + Sync {
    fn now(&self) -> UnixMillis;
}

/// 実装は暗号学的に安全な乱数を使う。試験実装は決定的な値を供給できる。
pub trait Random: Send + Sync {
    fn uuid_v7(&self) -> EntityId;
    fn opaque_token(&self) -> String;
}
