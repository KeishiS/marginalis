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

    for command in ["validate-archive", "verify-restore"] {
        let result = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
            .args([command, "--input"])
            .arg(&archive)
            .output()
            .expect("run archive validation");
        assert!(
            result.status.success(),
            "{command} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn backup_retention_only_removes_old_verified_successes() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "marginalis-cli-retention-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("test directory");
    let database = directory.join("marginalis.sqlite3");
    let database_url = format!("sqlite://{}?mode=rwc", database.display());

    for generation in [100_u64, 200, 300] {
        let output = directory.join(format!("backup-{generation}"));
        let result = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
            .args(["backup", "--output"])
            .arg(&output)
            .env("MARGINALIS_DATABASE_URL", &database_url)
            .output()
            .expect("run backup");
        assert!(result.status.success());
    }
    fs::create_dir(directory.join("backup-50")).expect("incomplete backup");
    fs::write(directory.join("operator-owned"), "keep").expect("unrelated file");

    let verify = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["verify-latest-backup", "--directory"])
        .arg(&directory)
        .output()
        .expect("verify latest backup");
    assert!(
        verify.status.success(),
        "latest verification failed: {}",
        String::from_utf8_lossy(&verify.stderr)
    );

    let prune = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["prune-backups", "--directory"])
        .arg(&directory)
        .args(["--keep", "2"])
        .output()
        .expect("prune backups");
    assert!(
        prune.status.success(),
        "prune failed: {}",
        String::from_utf8_lossy(&prune.stderr)
    );
    assert!(!directory.join("backup-100").exists());
    assert!(directory.join("backup-200").exists());
    assert!(directory.join("backup-300").exists());
    assert!(directory.join("backup-50").exists());
    assert!(directory.join("operator-owned").exists());

    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn corrupted_success_generation_prevents_retention_deletion() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "marginalis-cli-corrupt-retention-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("test directory");
    let database = directory.join("marginalis.sqlite3");
    let database_url = format!("sqlite://{}?mode=rwc", database.display());
    for generation in [100_u64, 200] {
        let result = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
            .args(["backup", "--output"])
            .arg(directory.join(format!("backup-{generation}")))
            .env("MARGINALIS_DATABASE_URL", &database_url)
            .output()
            .expect("run backup");
        assert!(result.status.success());
    }
    fs::write(
        directory.join("backup-200/marginalis-archive.json"),
        "{broken",
    )
    .expect("corrupt archive");

    let prune = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["prune-backups", "--directory"])
        .arg(&directory)
        .args(["--keep", "1"])
        .output()
        .expect("prune backups");
    assert!(!prune.status.success());
    assert!(directory.join("backup-100").exists());
    assert!(directory.join("backup-200").exists());

    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn corrupted_archive_and_failed_backup_are_not_accepted_as_successes() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "marginalis-cli-failure-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("test directory");

    let corrupt_archive = directory.join("corrupt.json");
    fs::write(&corrupt_archive, "{broken").expect("corrupt archive");
    for command in ["validate-archive", "verify-restore"] {
        let result = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
            .args([command, "--input"])
            .arg(&corrupt_archive)
            .output()
            .expect("run archive validation");
        assert!(!result.status.success(), "{command} accepted corrupt input");
    }

    let incomplete = directory.join("incomplete");
    let result = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["backup", "--output"])
        .arg(&incomplete)
        .env(
            "MARGINALIS_DATABASE_URL",
            format!(
                "sqlite://{}?mode=rwc",
                directory.join("missing/marginalis.sqlite").display()
            ),
        )
        .output()
        .expect("run failed backup");
    assert!(!result.status.success());
    assert!(incomplete.is_dir());
    assert!(!incomplete.join("COMPLETE").exists());

    fs::remove_dir_all(&directory).expect("remove test directory");
}
