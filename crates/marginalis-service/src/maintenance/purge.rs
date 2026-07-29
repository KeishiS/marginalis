//! 保持期限を過ぎた永続状態の削除。

use crate::{config::StorageConfig, runtime::SystemClock};
use marginalis_application::Clock;
use marginalis_domain::{SOFT_DELETE_RETENTION_MS, UnixMillis};
use marginalis_sqlite::SqliteDatabase;

/// 保持期限を過ぎたnoteと一時的な認証状態を物理削除する。
pub(crate) async fn purge_expired() -> Result<(), Box<dyn std::error::Error>> {
    purge_expired_state().await
}

async fn purge_expired_state() -> Result<(), Box<dyn std::error::Error>> {
    let configuration = StorageConfig::from_environment()?;
    let database = SqliteDatabase::connect(&configuration.database_url).await?;
    let now = SystemClock.now();
    let note_cutoff = UnixMillis::new(now.get().saturating_sub(SOFT_DELETE_RETENTION_MS));
    let note_count = database.purge_deleted_before(note_cutoff).await?;
    let auth_counts = database.purge_expired_auth_state(now).await?;
    tracing::info!(
        event = "maintenance.purge.completed",
        note_count,
        web_sessions = auth_counts.web_sessions,
        oidc_login_attempts = auth_counts.oidc_login_attempts,
        note_cutoff_ms = note_cutoff.get(),
        "purged expired persisted state"
    );
    Ok(())
}
