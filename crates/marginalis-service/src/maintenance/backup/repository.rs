//! Backup世代の探索、検証、保持。

use super::super::archive::{read_validated_archive, verify_archive_in_memory};
use std::{
    cmp::Reverse,
    path::{Path, PathBuf},
};

pub(super) fn canonical_directory(directory: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let canonical = directory.canonicalize()?;
    if !canonical.is_dir() {
        return Err("backup directory is not a directory".into());
    }
    Ok(canonical)
}

pub(super) async fn validated_successful_generations(
    canonical_directory: &Path,
) -> Result<Vec<(u128, PathBuf)>, Box<dyn std::error::Error>> {
    let mut successful = Vec::new();
    for entry in std::fs::read_dir(canonical_directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(generation) = name.strip_prefix("backup-") else {
            continue;
        };
        let Ok(generation) = generation.parse::<u128>() else {
            continue;
        };
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        let canonical_path = path.canonicalize()?;
        if canonical_path.parent() != Some(canonical_directory) {
            return Err(format!("backup generation escapes backup directory: {name}").into());
        }
        let marker = canonical_path.join("COMPLETE");
        if !marker
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_file())
        {
            continue;
        }
        let expected_marker = format!(
            "Marginalis backup {}\n",
            marginalis_asciidoc::ARCHIVE_FORMAT
        );
        if std::fs::read_to_string(&marker)? != expected_marker {
            continue;
        }
        let archive_path = canonical_path.join("marginalis-archive.json");
        if !archive_path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_file())
        {
            continue;
        }
        let archive = read_validated_archive(&archive_path)?;
        verify_archive_in_memory(&archive).await?;
        successful.push((generation, canonical_path));
    }
    successful.sort_by_key(|entry| Reverse(entry.0));
    Ok(successful)
}

pub(super) fn remove_expired_generations<E>(
    successful: Vec<(u128, PathBuf)>,
    keep: usize,
    mut remove: impl FnMut(&Path) -> Result<(), E>,
) -> Result<(), E> {
    // 最古の世代から削除し、失敗時にはそれより新しい世代をすべて残す。
    for (_, path) in successful.into_iter().skip(keep).rev() {
        remove(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::remove_expired_generations;
    use std::path::{Path, PathBuf};

    #[test]
    fn retention_stops_at_the_first_deletion_failure_and_preserves_newer_generations() {
        let generations = vec![
            (400, PathBuf::from("/backup/backup-400")),
            (300, PathBuf::from("/backup/backup-300")),
            (200, PathBuf::from("/backup/backup-200")),
            (100, PathBuf::from("/backup/backup-100")),
        ];
        let mut attempted = Vec::new();

        let result = remove_expired_generations(generations, 1, |path: &Path| {
            attempted.push(path.to_path_buf());
            if path.ends_with("backup-200") {
                Err("simulated deletion failure")
            } else {
                Ok(())
            }
        });

        assert_eq!(result, Err("simulated deletion failure"));
        assert_eq!(
            attempted,
            [
                PathBuf::from("/backup/backup-100"),
                PathBuf::from("/backup/backup-200")
            ]
        );
    }
}
