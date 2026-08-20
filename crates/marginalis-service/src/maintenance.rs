//! SQLite正本の保持、移行、backupを行う定期・運用保守command。

mod archive;
mod backup;
mod diagnostics;
mod migration;
mod purge;

pub(crate) use archive::{
    export_archive, export_documents, import_archive, import_documents, migrate_archive,
    restore_archive, validate_archive, verify_restore,
};
pub(crate) use backup::{backup, prune_backups, verify_latest_backup};
pub(crate) use diagnostics::diagnose;
pub(crate) use migration::migrate_database;
pub(crate) use purge::purge_expired;

use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

const PRIVATE_FILE_MODE: u32 = 0o600;
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;

fn sync_parent_directory(path: &Path) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("output path has no parent directory"))?;
    File::open(parent)?.sync_all()
}

/// 完成するまで最終パスへ現れない、上書き禁止の非公開出力。
struct PendingOutput {
    temporary: PathBuf,
    final_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
enum PendingOutputCommitError {
    #[error("最終出力を作成できませんでした")]
    NotPublished(#[source] io::Error),
    #[error("最終出力 {path} は作成されましたが、確定処理に失敗しました")]
    Published {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl PendingOutput {
    fn create(final_path: &Path) -> io::Result<(Self, File)> {
        let parent = final_path
            .parent()
            .ok_or_else(|| io::Error::other("output path has no parent directory"))?;
        let file_name = final_path
            .file_name()
            .ok_or_else(|| io::Error::other("output path has no file name"))?;
        for attempt in 0_u16..128 {
            let mut temporary_name = OsString::from(".");
            temporary_name.push(file_name);
            temporary_name.push(format!(".pending-{}-{attempt}", std::process::id()));
            let temporary = parent.join(temporary_name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(PRIVATE_FILE_MODE)
                .open(&temporary)
            {
                Ok(file) => {
                    return Ok((
                        Self {
                            temporary,
                            final_path: final_path.to_owned(),
                        },
                        file,
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a temporary output file",
        ))
    }

    fn path(&self) -> &Path {
        &self.temporary
    }

    fn commit(self) -> Result<(), PendingOutputCommitError> {
        File::open(&self.temporary)
            .and_then(|file| file.sync_all())
            .map_err(PendingOutputCommitError::NotPublished)?;
        std::fs::hard_link(&self.temporary, &self.final_path)
            .map_err(PendingOutputCommitError::NotPublished)?;
        sync_parent_directory(&self.final_path).map_err(|source| self.published_error(source))?;
        std::fs::remove_file(&self.temporary).map_err(|source| self.published_error(source))?;
        sync_parent_directory(&self.final_path).map_err(|source| self.published_error(source))?;
        Ok(())
    }

    fn published_error(&self, source: io::Error) -> PendingOutputCommitError {
        PendingOutputCommitError::Published {
            path: self.final_path.clone(),
            source,
        }
    }
}

impl Drop for PendingOutput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.temporary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_output_is_invisible_until_commit_and_never_overwrites() {
        let directory =
            std::env::temp_dir().join(format!("marginalis-pending-output-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir(&directory).expect("test directory");
        let output = directory.join("archive.json");

        let (pending, mut file) = PendingOutput::create(&output).expect("pending output");
        use std::io::Write as _;
        file.write_all(b"complete").expect("write output");
        drop(file);
        assert!(!output.exists());
        pending.commit().expect("commit output");
        assert_eq!(std::fs::read(&output).expect("read output"), b"complete");

        let (pending, _file) = PendingOutput::create(&output).expect("second pending output");
        assert_eq!(
            match pending.commit().expect_err("must not overwrite") {
                PendingOutputCommitError::NotPublished(error) => error.kind(),
                PendingOutputCommitError::Published { .. } => panic!("output was not replaced"),
            },
            io::ErrorKind::AlreadyExists,
        );
        assert_eq!(std::fs::read(&output).expect("read output"), b"complete");
        std::fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn unfinished_pending_output_is_removed() {
        let directory = std::env::temp_dir().join(format!(
            "marginalis-pending-output-drop-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir(&directory).expect("test directory");
        let output = directory.join("archive.json");
        let temporary = {
            let (pending, _file) = PendingOutput::create(&output).expect("pending output");
            pending.path().to_owned()
        };
        assert!(!temporary.exists());
        assert!(!output.exists());
        std::fs::remove_dir_all(directory).expect("remove test directory");
    }
}
