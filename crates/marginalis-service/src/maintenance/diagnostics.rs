//! SQLiteと公開設定の読み取り専用診断。

use marginalis_sqlite::SqliteDatabase;

#[derive(serde::Serialize)]
struct DiagnosticReport {
    status: &'static str,
    event: &'static str,
    database: marginalis_sqlite::SqliteDiagnosticReport,
    configuration: PublicConfigurationReport,
}

#[derive(serde::Serialize)]
struct PublicConfigurationReport {
    database_configured: bool,
    base_url: Option<String>,
    listen_address: Option<String>,
    oidc_issuer_url: Option<String>,
    oidc_client_id_configured: bool,
    oidc_ca_certificate_file: Option<String>,
    mcp_enabled: Option<bool>,
    mcp_allowed_origin_count: usize,
    mcp_authorization_configured: bool,
}

/// SQLiteと公開設定を変更せずに検査し、結果をJSONで出力する。
pub(crate) async fn diagnose() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = nonempty_environment_variable("MARGINALIS_DATABASE_URL");
    let database = match database_url.as_deref() {
        Some(database_url) => SqliteDatabase::diagnose(database_url).await,
        None => SqliteDatabase::diagnose("sqlite://configuration-is-missing?mode=ro").await,
    };
    let healthy = database.healthy();
    if !healthy {
        tracing::warn!(
            event = "maintenance.diagnostics.failed",
            database_available = database.available,
            schema_actual = ?database.schema.actual,
            schema_expected = ?database.schema.expected,
            integrity_actual = ?database.integrity.actual,
            foreign_key_violation_count = ?database.foreign_keys.actual,
            failures = ?database.failures,
            "SQLite diagnostics reported an unhealthy database"
        );
    }
    let report = DiagnosticReport {
        status: if healthy { "ok" } else { "failed" },
        event: "diagnostics.completed",
        database,
        configuration: public_configuration(),
    };
    serde_json::to_writer(std::io::stdout().lock(), &report)?;
    println!();
    if healthy {
        Ok(())
    } else {
        Err("diagnostics reported an unhealthy database".into())
    }
}

fn public_configuration() -> PublicConfigurationReport {
    let mcp_allowed_origin_count = std::env::var("MARGINALIS_MCP_ALLOWED_ORIGINS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter(|origin| !origin.trim().is_empty())
                .count()
        })
        .unwrap_or(0);
    PublicConfigurationReport {
        database_configured: nonempty_environment_variable("MARGINALIS_DATABASE_URL").is_some(),
        base_url: nonempty_environment_variable("MARGINALIS_BASE_URL"),
        listen_address: nonempty_environment_variable("MARGINALIS_LISTEN_ADDR"),
        oidc_issuer_url: nonempty_environment_variable("OIDC_ISSUER_URL"),
        oidc_client_id_configured: nonempty_environment_variable("OIDC_CLIENT_ID").is_some(),
        oidc_ca_certificate_file: nonempty_environment_variable("OIDC_CA_CERTIFICATE_FILE"),
        mcp_enabled: std::env::var("MARGINALIS_MCP_ENABLE")
            .ok()
            .and_then(|value| value.parse().ok()),
        mcp_allowed_origin_count,
        mcp_authorization_configured: nonempty_environment_variable(
            "MARGINALIS_MCP_AUTHORIZATION_ISSUER",
        )
        .is_some(),
    }
}

fn nonempty_environment_variable(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}
