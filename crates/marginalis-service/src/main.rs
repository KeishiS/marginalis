//! Marginalisのcomposition root。commandの選択とprocess lifecycleだけを担う。

mod cli;
mod config;
mod environment;
mod maintenance;
mod mcp_client_metadata;
mod runtime;
mod serve;

use tracing_subscriber::EnvFilter;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Command {
    Serve,
    Diagnose,
    PurgeExpired,
    ExportArchive,
    ExportDocuments,
    ImportDocuments,
    MigrateArchive,
    ImportArchive,
    ValidateArchive,
    VerifyRestore,
    VerifyLatestBackup,
    Backup,
    PruneBackups,
    Unknown,
}

impl Command {
    fn parse(value: Option<&str>) -> Self {
        match value {
            None | Some("serve") => Self::Serve,
            Some("diagnose") => Self::Diagnose,
            Some("purge-expired") => Self::PurgeExpired,
            Some("export-archive") => Self::ExportArchive,
            Some("export-documents") => Self::ExportDocuments,
            Some("import-documents") => Self::ImportDocuments,
            Some("migrate-archive") => Self::MigrateArchive,
            Some("import-archive") => Self::ImportArchive,
            Some("validate-archive") => Self::ValidateArchive,
            Some("verify-restore") => Self::VerifyRestore,
            Some("verify-latest-backup") => Self::VerifyLatestBackup,
            Some("backup") => Self::Backup,
            Some("prune-backups") => Self::PruneBackups,
            Some(_) => Self::Unknown,
        }
    }

    fn log_failure(self, error: &dyn std::error::Error) {
        match self {
            Self::Serve => tracing::error!(
                event = "service.failed",
                command = "serve",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::Diagnose => tracing::error!(
                event = "maintenance.diagnostics.failed",
                command = "diagnose",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::PurgeExpired => tracing::error!(
                event = "maintenance.purge.failed",
                command = "purge-expired",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::ExportArchive => tracing::error!(
                event = "maintenance.archive_export.failed",
                command = "export-archive",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::ExportDocuments => tracing::error!(
                event = "maintenance.document_export.failed",
                command = "export-documents",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::ImportDocuments => tracing::error!(
                event = "maintenance.document_import.failed",
                command = "import-documents",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::MigrateArchive => tracing::error!(
                event = "maintenance.archive_migration.failed",
                command = "migrate-archive",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::ImportArchive => tracing::error!(
                event = "maintenance.archive_import.failed",
                command = "import-archive",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::ValidateArchive => tracing::error!(
                event = "maintenance.archive_validation.failed",
                command = "validate-archive",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::VerifyRestore => tracing::error!(
                event = "maintenance.restore_verification.failed",
                command = "verify-restore",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::VerifyLatestBackup => tracing::error!(
                event = "maintenance.backup_verification.failed",
                command = "verify-latest-backup",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::Backup => tracing::error!(
                event = "maintenance.backup.failed",
                command = "backup",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::PruneBackups => tracing::error!(
                event = "maintenance.backup_prune.failed",
                command = "prune-backups",
                error = %error,
                "Marginalis command terminated"
            ),
            Self::Unknown => tracing::error!(
                event = "command.failed",
                command = "unknown",
                error = %error,
                "Marginalis command terminated"
            ),
        }
    }
}

#[tokio::main]
async fn main() {
    let mut arguments = std::env::args().skip(1);
    let command_argument = arguments.next();
    if matches!(command_argument.as_deref(), Some("--version" | "-V")) && arguments.next().is_none()
    {
        println!("marginalis {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    initialize_tracing();
    let command = Command::parse(command_argument.as_deref());
    let result = match command {
        Command::Serve => serve::run().await,
        Command::Diagnose if arguments.next().is_none() => maintenance::diagnose().await,
        Command::Diagnose => Err(cli::USAGE.into()),
        Command::PurgeExpired => maintenance::purge_expired().await,
        Command::ExportArchive => maintenance::export_archive(arguments).await,
        Command::ExportDocuments => maintenance::export_documents(arguments).await,
        Command::ImportDocuments => maintenance::import_documents(arguments).await,
        Command::MigrateArchive => maintenance::migrate_archive(arguments).await,
        Command::ImportArchive => maintenance::import_archive(arguments).await,
        Command::ValidateArchive => maintenance::validate_archive(arguments).await,
        Command::VerifyRestore => maintenance::verify_restore(arguments).await,
        Command::VerifyLatestBackup => maintenance::verify_latest_backup(arguments).await,
        Command::Backup => maintenance::backup(arguments).await,
        Command::PruneBackups => maintenance::prune_backups(arguments).await,
        Command::Unknown => Err(cli::USAGE.into()),
    };
    if let Err(error) = result {
        command.log_failure(error.as_ref());
        std::process::exit(1);
    }
}

fn initialize_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,marginalis_auth_oidc=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .compact()
        .init();
}

#[cfg(test)]
mod tests {
    use super::Command;

    #[test]
    fn command_parsing_normalizes_unknown_values() {
        assert_eq!(Command::parse(Some("backup")), Command::Backup);
        assert_eq!(Command::parse(Some("secret-value")), Command::Unknown);
        assert_eq!(Command::parse(None), Command::Serve);
    }

    #[test]
    fn openapi_identity_matches_the_configured_note_profile() {
        let document = marginalis_contract::openapi_document();
        assert_eq!(
            document["info"]["x-adocweave-package-version"],
            marginalis_asciidoc::PINNED_ADOCWEAVE_PACKAGE_VERSION
        );
        assert_eq!(
            document["info"]["x-note-profile-version"],
            marginalis_asciidoc::AUTHORING_PROFILE_VERSION
        );
    }
}
