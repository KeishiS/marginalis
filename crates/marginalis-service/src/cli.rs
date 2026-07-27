//! 保守commandに共通する引数仕様。

use std::path::PathBuf;

pub(crate) const USAGE: &str = "usage: marginalis [--version|serve|diagnose|purge-expired|export-archive --output <absolute-file>|import-archive --input <absolute-file>|validate-archive --input <absolute-file>|verify-restore --input <absolute-file>|verify-latest-backup --directory <absolute-directory>|backup (--output <absolute-directory>|--directory <absolute-directory>)|prune-backups --directory <absolute-directory> --keep <positive-count>]";

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
}
