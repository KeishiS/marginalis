//! OIDC issuer移行時に使う、明示的な外部identityの引き継ぎcommand。

use std::{collections::BTreeMap, path::PathBuf};

use marginalis_application::Clock as _;
use marginalis_domain::Identity;
use marginalis_sqlite::{IdentityMaintenanceRequest, SqliteDatabase};

use crate::{config::StorageConfig, runtime::SystemClock};

pub(crate) async fn link_identity(
    arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut parsed = IdentityArguments::parse(arguments, true)?;
    let existing = Identity::new(
        parsed.take("--existing-issuer")?,
        parsed.take("--existing-subject")?,
    )?;
    let new_identity = Identity::new(parsed.take("--new-issuer")?, parsed.take("--new-subject")?)?;
    let make_primary = parsed.flag("--make-primary");
    let backup_path = parsed.backup_path("identity-link")?;
    parsed.finish()?;

    let configuration = StorageConfig::from_environment()?;
    let report = SqliteDatabase::maintain_identity(
        &configuration.database_url,
        &backup_path,
        IdentityMaintenanceRequest::Link {
            existing,
            new_identity,
            make_primary,
        },
    )
    .await?;
    tracing::info!(
        event = "maintenance.identity_link.completed",
        backup = %report.backup_path.display(),
        primary_changed = report.primary_changed,
        "identity link completed"
    );
    Ok(())
}

pub(crate) async fn set_primary_identity(
    arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut parsed = IdentityArguments::parse(arguments, false)?;
    let identity = Identity::new(parsed.take("--issuer")?, parsed.take("--subject")?)?;
    let backup_path = parsed.backup_path("identity-primary")?;
    parsed.finish()?;

    let configuration = StorageConfig::from_environment()?;
    let report = SqliteDatabase::maintain_identity(
        &configuration.database_url,
        &backup_path,
        IdentityMaintenanceRequest::SetPrimary { identity },
    )
    .await?;
    tracing::info!(
        event = "maintenance.identity_primary.completed",
        backup = %report.backup_path.display(),
        "primary identity change completed"
    );
    Ok(())
}

struct IdentityArguments {
    values: BTreeMap<String, String>,
    make_primary: bool,
}

impl IdentityArguments {
    fn parse(
        arguments: impl Iterator<Item = String>,
        allow_make_primary: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut arguments = arguments.peekable();
        let mut values = BTreeMap::new();
        let mut make_primary = false;
        while let Some(option) = arguments.next() {
            if option == "--make-primary" {
                if !allow_make_primary || make_primary {
                    return Err("identity maintenance options are invalid".into());
                }
                make_primary = true;
                continue;
            }
            if !option.starts_with("--") || values.contains_key(&option) {
                return Err("identity maintenance options are invalid".into());
            }
            let value = arguments
                .next()
                .ok_or("identity maintenance option requires a value")?;
            values.insert(option, value);
        }
        Ok(Self {
            values,
            make_primary,
        })
    }

    fn take(&mut self, option: &str) -> Result<String, Box<dyn std::error::Error>> {
        self.values
            .remove(option)
            .ok_or_else(|| format!("identity maintenance requires {option}").into())
    }

    fn flag(&self, option: &str) -> bool {
        option == "--make-primary" && self.make_primary
    }

    fn backup_path(&mut self, prefix: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        match (
            self.values.remove("--backup-output"),
            self.values.remove("--backup-directory"),
        ) {
            (Some(path), None) => {
                let path = PathBuf::from(path);
                if !path.is_absolute() {
                    return Err("identity maintenance backup output must be absolute".into());
                }
                Ok(path)
            }
            (None, Some(path)) => {
                let directory = PathBuf::from(path);
                if !directory.is_absolute() || !directory.is_dir() {
                    return Err(
                        "identity maintenance backup directory must be an existing absolute directory"
                            .into(),
                    );
                }
                Ok(directory.join(format!(
                    "{prefix}-{}.sqlite3",
                    SystemClock.now().get()
                )))
            }
            _ => Err(
                "identity maintenance requires exactly one of --backup-output or --backup-directory"
                    .into(),
            ),
        }
    }

    fn finish(self) -> Result<(), Box<dyn std::error::Error>> {
        if self.values.is_empty() {
            Ok(())
        } else {
            Err("identity maintenance options are invalid".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_arguments_are_order_independent_but_reject_duplicates_and_unknown_options() {
        let directory = std::env::temp_dir();
        let arguments = [
            "--new-subject",
            "alice-v2",
            "--existing-subject",
            "alice",
            "--make-primary",
            "--backup-directory",
            directory.to_str().expect("temporary directory"),
            "--new-issuer",
            "https://new-id.example.test",
            "--existing-issuer",
            "https://old-id.example.test",
        ]
        .into_iter()
        .map(str::to_owned);
        let mut parsed = IdentityArguments::parse(arguments, true).expect("arguments");
        assert_eq!(parsed.take("--existing-subject").unwrap(), "alice");
        assert_eq!(parsed.take("--new-subject").unwrap(), "alice-v2");
        assert!(parsed.flag("--make-primary"));
        assert!(parsed.backup_path("identity-link").unwrap().is_absolute());

        let duplicate = ["--issuer", "one", "--issuer", "two"]
            .into_iter()
            .map(str::to_owned);
        assert!(IdentityArguments::parse(duplicate, false).is_err());
    }
}
