use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn test_directory(purpose: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "marginalis-cli-{purpose}-{}-{unique}",
        std::process::id()
    ))
}

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
    let directory = test_directory("permissions");
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
    let archive_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&archive).expect("read archive")).expect("archive JSON");
    assert_eq!(archive_json["format"], "marginalis-archive-8");
    assert_eq!(archive_json["adocweave_package_version"], "0.17.0");
    assert_eq!(archive_json["note_profile_version"], 4);

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
        let event = match command {
            "validate-archive" => "maintenance.archive_validation.completed",
            "verify-restore" => "maintenance.restore_verification.completed",
            _ => unreachable!(),
        };
        assert!(String::from_utf8_lossy(&result.stderr).contains(event));
    }

    let incompatible_archive = directory.join("incompatible.json");
    let mut incompatible_json = archive_json.clone();
    incompatible_json["adocweave_package_version"] = "0.10.1".into();
    fs::write(
        &incompatible_archive,
        serde_json::to_vec(&incompatible_json).expect("serialize incompatible archive"),
    )
    .expect("write incompatible archive");
    let result = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["validate-archive", "--input"])
        .arg(&incompatible_archive)
        .output()
        .expect("validate incompatible archive");
    assert!(!result.status.success());
    let incompatible_target = directory.join("incompatible-target.sqlite");
    let result = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["import-archive", "--input"])
        .arg(&incompatible_archive)
        .env(
            "MARGINALIS_DATABASE_URL",
            format!("sqlite://{}?mode=rwc", incompatible_target.display()),
        )
        .output()
        .expect("import incompatible archive");
    assert!(!result.status.success());
    assert!(
        !incompatible_target.exists(),
        "incompatible archive must be rejected before opening the target database"
    );

    let previous_archive = directory.join("previous-format.json");
    let mut previous_json = archive_json.clone();
    previous_json["format"] = "marginalis-archive-3".into();
    fs::write(
        &previous_archive,
        serde_json::to_vec(&previous_json).expect("serialize previous archive"),
    )
    .expect("write previous archive");
    let result = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["validate-archive", "--input"])
        .arg(&previous_archive)
        .output()
        .expect("validate previous archive");
    assert!(!result.status.success());

    let unknown_field_archive = directory.join("unknown-field.json");
    incompatible_json["adocweave_package_version"] = "0.17.0".into();
    incompatible_json["unexpected"] = true.into();
    fs::write(
        &unknown_field_archive,
        serde_json::to_vec(&incompatible_json).expect("serialize archive with unknown field"),
    )
    .expect("write archive with unknown field");
    let result = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["validate-archive", "--input"])
        .arg(&unknown_field_archive)
        .output()
        .expect("validate archive with unknown field");
    assert!(!result.status.success());

    let mut nested_base = archive_json.clone();
    nested_base["notes"] = serde_json::json!([{
        "note_id": "0197c9bc-0000-7000-8000-000000000001",
        "creator_issuer": "https://id.example.test",
        "creator_subject": "alice",
        "title": "Title",
        "body": "Body.",
        "tags": [],
        "created_at": 0,
        "updated_at": 0,
        "revision": 1,
        "deleted_at": null
    }]);
    let mut unknown_note_json = nested_base.clone();
    unknown_note_json["notes"][0]["unexpected"] = true.into();
    let unknown_note_archive = directory.join("unknown-note-field.json");
    fs::write(
        &unknown_note_archive,
        serde_json::to_vec(&unknown_note_json).expect("serialize unknown note field"),
    )
    .expect("write unknown note field");
    let result = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["validate-archive", "--input"])
        .arg(&unknown_note_archive)
        .output()
        .expect("validate unknown note field");
    assert!(
        !result.status.success(),
        "unknown note field must be rejected"
    );

    let mut old_bundle_json = nested_base;
    old_bundle_json["notes"][0] = serde_json::json!({
        "note": old_bundle_json["notes"][0].clone(),
        "acl": []
    });
    let old_bundle_archive = directory.join("old-bundle-shape.json");
    fs::write(
        &old_bundle_archive,
        serde_json::to_vec(&old_bundle_json).expect("serialize old bundle shape"),
    )
    .expect("write old bundle shape");
    let result = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["validate-archive", "--input"])
        .arg(&old_bundle_archive)
        .output()
        .expect("validate old bundle shape");
    assert!(
        !result.status.success(),
        "old bundle shape must be rejected"
    );

    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn archive_migration_revalidates_all_notes_and_preserves_the_input() {
    let directory = test_directory("archive-migration");
    fs::create_dir(&directory).expect("test directory");
    let input = directory.join("archive-7.json");
    let output = directory.join("archive-8.json");
    let previous = serde_json::json!({
        "format": "marginalis-archive-7",
        "adocweave_package_version": "0.11.0",
        "note_profile_version": 3,
        "notes": [{
            "note_id": "0197c9bc-0000-7000-8000-000000000001",
            "creator_issuer": "https://id.example.test",
            "creator_subject": "alice",
            "source": "= Note\n:source-language: rust\n:tags: {source-language}\n\nbody",
            "created_at_ms": 1,
            "updated_at_ms": 2,
            "revision": 1,
            "deleted_at_ms": null
        }],
        "note_acl": []
    });
    let input_bytes = serde_json::to_vec_pretty(&previous).expect("previous archive");
    fs::write(&input, &input_bytes).expect("write previous archive");

    let migration = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["migrate-archive", "--input"])
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .output()
        .expect("migrate archive");
    assert!(
        migration.status.success(),
        "migration failed: {}",
        String::from_utf8_lossy(&migration.stderr)
    );
    assert_eq!(fs::read(&input).expect("unchanged input"), input_bytes);
    assert_eq!(
        fs::metadata(&output)
            .expect("migration output")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let migrated: serde_json::Value =
        serde_json::from_slice(&fs::read(&output).expect("read migration output"))
            .expect("migrated JSON");
    assert_eq!(migrated["format"], "marginalis-archive-8");
    assert_eq!(migrated["adocweave_package_version"], "0.17.0");
    assert_eq!(migrated["note_profile_version"], 4);

    let invalid_input = directory.join("invalid-archive-7.json");
    let invalid_output = directory.join("invalid-archive-8.json");
    let mut invalid = previous;
    invalid["notes"][0]["source"] =
        concat!("= Note\n:tags: research, + \\", "\n  rust\n\nbody").into();
    fs::write(
        &invalid_input,
        serde_json::to_vec_pretty(&invalid).expect("invalid previous archive"),
    )
    .expect("write invalid previous archive");
    let rejected = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["migrate-archive", "--input"])
        .arg(&invalid_input)
        .arg("--output")
        .arg(&invalid_output)
        .output()
        .expect("reject archive migration");
    assert!(!rejected.status.success());
    assert!(!invalid_output.exists());
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(stderr.contains("maintenance.archive_migration.failed"));
    assert!(stderr.contains("archive note at position 1"));
    assert!(!stderr.contains("research"));

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
    assert!(
        String::from_utf8_lossy(&verify.stderr)
            .contains("maintenance.backup_verification.completed")
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
    assert!(String::from_utf8_lossy(&prune.stderr).contains("maintenance.backup_prune.completed"));
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
    assert!(String::from_utf8_lossy(&prune.stderr).contains("maintenance.backup_prune.failed"));
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
        let event = match command {
            "validate-archive" => "maintenance.archive_validation.failed",
            "verify-restore" => "maintenance.restore_verification.failed",
            _ => unreachable!(),
        };
        assert!(String::from_utf8_lossy(&result.stderr).contains(event));
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

#[test]
fn diagnose_reports_a_healthy_database_as_json_without_secrets() {
    let directory = test_directory("diagnostics");
    fs::create_dir(&directory).expect("test directory");
    let database = directory.join("marginalis.sqlite3");
    let database_url = format!("sqlite://{}?mode=rwc", database.display());
    let archive = directory.join("archive.json");

    let initialize = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["export-archive", "--output"])
        .arg(&archive)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("initialize database");
    assert!(initialize.status.success());

    let healthy = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .arg("diagnose")
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .env("OIDC_CLIENT_SECRET", "must-not-be-reported")
        .output()
        .expect("diagnose database");
    assert!(healthy.status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&healthy.stdout).expect("diagnostic JSON");
    assert_eq!(report["status"], "ok");
    assert_eq!(report["database"]["schema"]["actual"], 11);
    assert!(!String::from_utf8_lossy(&healthy.stdout).contains("must-not-be-reported"));
    assert!(!String::from_utf8_lossy(&healthy.stderr).contains("must-not-be-reported"));

    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn diagnose_reports_an_unavailable_database_and_fails() {
    let directory = test_directory("unavailable-diagnostics");
    let database_url = format!("sqlite://{}/missing.sqlite3?mode=ro", directory.display());

    let output = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .arg("diagnose")
        .env("MARGINALIS_DATABASE_URL", database_url)
        .env("OIDC_CLIENT_SECRET", "must-not-be-reported")
        .output()
        .expect("diagnose unavailable database");

    assert!(!output.status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("diagnostic JSON");
    assert_eq!(report["status"], "failed");
    assert_eq!(report["database"]["available"], false);
    assert_eq!(report["database"]["error"], "connection_failed");
    assert_eq!(report["database"]["failures"][0]["check"], "connection");
    assert!(String::from_utf8_lossy(&output.stderr).contains("maintenance.diagnostics.failed"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("must-not-be-reported"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("must-not-be-reported"));
}
