use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn version_flags_report_the_packaged_version() {
    for flag in ["--version", "-V"] {
        let output = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
            .arg(flag)
            .output()
            .expect("run marginalis");

        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).expect("UTF-8 stdout"),
            format!("marginalis {}\n", env!("CARGO_PKG_VERSION"))
        );
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn archive_commands_create_private_outputs_without_relying_on_umask() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "marginalis-cli-permissions-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("test directory");
    let database = directory.join("marginalis.sqlite3");
    let database_url = format!("sqlite://{}?mode=rwc", database.display());
    let archive = directory.join("archive.json");

    let export = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["export-archive", "--output"])
        .arg(&archive)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("run archive export");
    assert!(
        export.status.success(),
        "archive export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    assert_eq!(
        fs::metadata(&archive)
            .expect("archive metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let backup = directory.join("backup");
    let result = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["backup", "--output"])
        .arg(&backup)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("run backup");
    assert!(
        result.status.success(),
        "backup failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::metadata(&backup)
            .expect("backup metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for name in ["marginalis-archive.json", "COMPLETE"] {
        assert_eq!(
            fs::metadata(backup.join(name))
                .expect("backup file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    fs::remove_dir_all(&directory).expect("remove test directory");
}
