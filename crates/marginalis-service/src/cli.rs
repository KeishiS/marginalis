//! 保守commandに共通する引数仕様。

use std::path::PathBuf;

pub(crate) const USAGE: &str = "usage: marginalis [--version|serve|diagnose|purge-expired|migrate-database (--output <absolute-file>|--directory <absolute-directory>)|export-archive --output <absolute-file>|export-documents --output <absolute-file>|import-documents --input <absolute-file>|migrate-archive --input <absolute-file> --output <absolute-file>|import-archive --input <absolute-file>|restore-archive --input <absolute-file>|validate-archive --input <absolute-file>|verify-restore --input <absolute-file>|verify-latest-backup --directory <absolute-directory>|backup (--output <absolute-directory>|--directory <absolute-directory>)|prune-backups --directory <absolute-directory> --keep <positive-count>]";

pub(crate) fn required_absolute_file_argument(
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let received_option = arguments.next();
    let value = arguments.next();
    if received_option.as_deref() != Some(option) || value.is_none() || arguments.next().is_some() {
        return Err(format!("usage requires {option} <absolute-file>").into());
    }
    let path = PathBuf::from(value.expect("value was checked"));
    if !path.is_absolute() {
        return Err(format!("{option} must be an absolute file path").into());
    }
    Ok(path)
}

pub(crate) fn required_archive_migration_arguments(
    arguments: &mut impl Iterator<Item = String>,
) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let input_option = arguments.next();
    let input = arguments.next();
    let output_option = arguments.next();
    let output = arguments.next();
    if input_option.as_deref() != Some("--input")
        || output_option.as_deref() != Some("--output")
        || input.is_none()
        || output.is_none()
        || arguments.next().is_some()
    {
        return Err("usage requires --input <absolute-file> --output <absolute-file>".into());
    }
    let input = PathBuf::from(input.expect("input was checked"));
    let output = PathBuf::from(output.expect("output was checked"));
    if !input.is_absolute() || !output.is_absolute() {
        return Err("archive migration paths must be absolute".into());
    }
    if input == output {
        return Err("archive migration output must differ from its input".into());
    }
    Ok((input, output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_arguments_require_exactly_one_absolute_file() {
        assert_eq!(
            required_absolute_file_argument(
                &mut [
                    "--output".to_owned(),
                    "/var/backups/archive.json".to_owned()
                ]
                .into_iter(),
                "--output",
            )
            .expect("absolute output"),
            PathBuf::from("/var/backups/archive.json")
        );
        assert!(
            required_absolute_file_argument(
                &mut ["--output".to_owned(), "relative.json".to_owned()].into_iter(),
                "--output",
            )
            .is_err()
        );
        assert!(
            required_absolute_file_argument(&mut ["--input".to_owned()].into_iter(), "--output")
                .is_err()
        );
    }

    #[test]
    fn archive_migration_requires_distinct_absolute_input_and_output() {
        assert_eq!(
            required_archive_migration_arguments(
                &mut [
                    "--input".to_owned(),
                    "/var/backups/archive-7.json".to_owned(),
                    "--output".to_owned(),
                    "/var/backups/archive-8.json".to_owned(),
                ]
                .into_iter(),
            )
            .expect("migration paths"),
            (
                PathBuf::from("/var/backups/archive-7.json"),
                PathBuf::from("/var/backups/archive-8.json")
            )
        );
        assert!(
            required_archive_migration_arguments(
                &mut [
                    "--input".to_owned(),
                    "/same.json".to_owned(),
                    "--output".to_owned(),
                    "/same.json".to_owned(),
                ]
                .into_iter(),
            )
            .is_err()
        );
    }
}
