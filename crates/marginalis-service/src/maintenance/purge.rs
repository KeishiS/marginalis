//! 保持期限を過ぎた永続状態の削除。

use crate::{config::StorageConfig, runtime::SystemClock};
use marginalis_application::Clock;
use marginalis_domain::{
    SOFT_DELETE_RETENTION_MS, UNREFERENCED_ATTACHMENT_RETENTION_MS, UnixMillis,
};
use marginalis_sqlite::SqliteDatabase;

const UNUSED_MCP_CLIENT_RETENTION_MS: i64 = 24 * 60 * 60 * 1_000;

/// 保持期限を過ぎたnote、未参照の添付画像、一時的な認証状態を物理削除する。
pub(crate) async fn purge_expired() -> Result<(), Box<dyn std::error::Error>> {
    purge_expired_state().await
}

async fn purge_expired_state() -> Result<(), Box<dyn std::error::Error>> {
    let configuration = StorageConfig::from_environment()?;
    let database = SqliteDatabase::connect(&configuration.database_url).await?;
    let now = SystemClock.now();
    let note_cutoff = UnixMillis::new(now.get().saturating_sub(SOFT_DELETE_RETENTION_MS));
    let note_count = database.purge_deleted_before(note_cutoff).await?;
    let attachment_cutoff = UnixMillis::new(
        now.get()
            .saturating_sub(UNREFERENCED_ATTACHMENT_RETENTION_MS),
    );
    let unreferenced_attachment_count = database
        .purge_unreferenced_note_attachments_before(attachment_cutoff)
        .await?;
    let webhook_deliveries =
        marginalis_application::WebhookDeliveryRepository::purge_expired_deliveries(&database, now)
            .await?;
    let operational_counts = database
        .purge_expired_operational_state(
            now,
            UnixMillis::new(now.get().saturating_sub(UNUSED_MCP_CLIENT_RETENTION_MS)),
        )
        .await?;
    let access_credentials = operational_counts.mcp_access_tokens;
    let refresh_credentials = operational_counts.mcp_refresh_tokens;
    let authorization_grants = operational_counts.mcp_authorization_codes;
    tracing::info!(
        event = "maintenance.purge.completed",
        note_count,
        unreferenced_attachment_count,
        web_sessions = operational_counts.web_sessions,
        oidc_login_attempts = operational_counts.oidc_login_attempts,
        mcp_access_credentials = access_credentials,
        mcp_refresh_credentials = refresh_credentials,
        mcp_authorization_grants = authorization_grants,
        mcp_client_authorizations = operational_counts.mcp_client_authorizations,
        mcp_clients = operational_counts.mcp_clients,
        note_sync_cursors = operational_counts.note_sync_cursors,
        note_sync_projection_entries = operational_counts.note_sync_projection_entries,
        webhook_deliveries,
        note_cutoff_ms = note_cutoff.get(),
        attachment_cutoff_ms = attachment_cutoff.get(),
        "purged expired persisted state"
    );
    Ok(())
}
