//! SQLite正本の保持、移行、backupを行う定期・運用保守command。

mod archive;
mod backup;
mod diagnostics;
mod purge;

pub(crate) use archive::{
    export_archive, export_documents, import_archive, migrate_archive, validate_archive,
    verify_restore,
};
pub(crate) use backup::{backup, prune_backups, verify_latest_backup};
pub(crate) use diagnostics::diagnose;
pub(crate) use purge::purge_expired;

use std::{fs::File, path::Path};

const PRIVATE_FILE_MODE: u32 = 0o600;
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;

fn sync_parent_directory(path: &Path) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("output path has no parent directory"))?;
    File::open(parent)?.sync_all()
}
