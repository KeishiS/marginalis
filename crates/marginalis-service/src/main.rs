//! Marginalisのcomposition root。commandの選択とprocess lifecycleだけを担う。

mod cli;
mod config;
mod maintenance;
mod runtime;
mod serve;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next();
    if matches!(command.as_deref(), Some("--version" | "-V")) && arguments.next().is_none() {
        println!("marginalis {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    initialize_tracing();
    let result = match command.as_deref() {
        None | Some("serve") => serve::run().await,
        Some("diagnose") if arguments.next().is_none() => maintenance::diagnose().await,
        Some("purge-expired") => maintenance::purge_expired().await,
        Some("export-archive") => maintenance::export_archive(arguments).await,
        Some("import-archive") => maintenance::import_archive(arguments).await,
        Some("validate-archive") => maintenance::validate_archive(arguments).await,
        Some("verify-restore") => maintenance::verify_restore(arguments).await,
        Some("verify-latest-backup") => maintenance::verify_latest_backup(arguments).await,
        Some("backup") => maintenance::backup(arguments).await,
        Some("prune-backups") => maintenance::prune_backups(arguments).await,
        Some(_) => Err(cli::USAGE.into()),
    };
    if let Err(error) = result {
        let command = command.as_deref().unwrap_or("serve");
        let event = match command {
            "validate-archive" => "maintenance.archive_validation.failed",
            "verify-restore" => "maintenance.restore_verification.failed",
            "verify-latest-backup" => "maintenance.backup_verification.failed",
            "prune-backups" => "maintenance.backup_prune.failed",
            _ => "command.failed",
        };
        tracing::error!(
            event,
            command,
            error = %error,
            "Marginalis command terminated"
        );
        std::process::exit(1);
    }
}

fn initialize_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,marginalis_auth_oidc=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .compact()
        .init();
}
