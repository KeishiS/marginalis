use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const PREVIOUS_PUBLISHED_ADOCWEAVE_VERSION: &str = "0.41.0";

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

fn current_archive_with_imported_history(mut archive: serde_json::Value) -> serde_json::Value {
    let histories = archive["notes"]
        .as_array()
        .expect("archive notes")
        .iter()
        .map(|note| {
            serde_json::json!({
                "note_id": note["note_id"],
                "revision": note["revision"],
                "changed_at_ms": note["updated_at_ms"],
                "changed_by_issuer": note["creator_issuer"],
                "changed_by_subject": note["creator_subject"],
                "kind": "imported",
                "source": note["source"],
                "deleted_at_ms": note["deleted_at_ms"],
                "review_tracking_known": note["provenance"]["review_tracking_known"],
                "reviewed_revision": note["provenance"]["reviewed_revision"],
                "reviewed_at_ms": note["provenance"]["reviewed_at_ms"],
                "reviewer_issuer": note["provenance"]["reviewer_issuer"],
                "reviewer_subject": note["provenance"]["reviewer_subject"],
                "attachment_ids": []
            })
        })
        .collect();
    archive
        .as_object_mut()
        .expect("archive object")
        .insert("note_revisions".into(), serde_json::Value::Array(histories));
    archive
        .as_object_mut()
        .expect("archive object")
        .insert("attachments".into(), serde_json::Value::Array(Vec::new()));
    archive
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
fn unknown_command_is_normalized_before_logging() {
    let secret_command = "secret-command-value";
    let output = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .arg(secret_command)
        .output()
        .expect("run marginalis");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(stderr.contains("event=\"command.failed\""));
    assert!(stderr.contains("command=\"unknown\""));
    assert!(!stderr.contains(secret_command));
}

#[test]
fn database_migration_of_the_current_schema_is_a_logged_no_op() {
    let directory = test_directory("database-migration-current");
    fs::create_dir(&directory).expect("test directory");
    let database = directory.join("marginalis.sqlite3");
    let database_url = format!("sqlite://{}?mode=rwc", database.display());
    let archive = directory.join("initialize.json");
    let backup = directory.join("database-migration.sqlite3");

    let initialize = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["export-archive", "--output"])
        .arg(&archive)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("initialize database");
    assert!(initialize.status.success());

    let migrated = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["migrate-database", "--output"])
        .arg(&backup)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("run database migration");

    assert!(
        migrated.status.success(),
        "migration failed: {}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    let stderr = String::from_utf8(migrated.stderr).expect("UTF-8 stderr");
    assert!(stderr.contains("event=\"maintenance.database_migration.completed\""));
    assert!(stderr.contains("from_schema=23"));
    assert!(stderr.contains("to_schema=23"));
    assert!(stderr.contains("applied_migrations=0"));
    assert!(!backup.exists());

    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn identity_maintenance_links_and_switches_aliases_without_logging_them() {
    let directory = test_directory("identity-maintenance");
    fs::create_dir(&directory).expect("test directory");
    let database = directory.join("marginalis.sqlite3");
    let database_url = format!("sqlite://{}?mode=rwc", database.display());
    let seed = directory.join("seed.json");
    let old_issuer = "https://old-id.example.test";
    let old_subject = "private-alice";
    let new_issuer = "https://new-id.example.test";
    let new_subject = "private-alice-v2";
    let archive = current_archive_with_imported_history(serde_json::json!({
        "format": "marginalis-archive-18",
        "adocweave_package_version": marginalis_asciidoc::PINNED_ADOCWEAVE_PACKAGE_VERSION,
        "note_profile_version": 6,
        "principals": [{
            "primary_issuer": old_issuer,
            "primary_subject": old_subject,
            "aliases": []
        }, {
            "primary_issuer": "https://id.example.test",
            "primary_subject": "bob",
            "aliases": []
        }],
        "notes": [],
        "note_acl": [],
        "bibliography_items": [],
        "bibliography_import_sources": [],
        "bibliography_import_links": [],
        "math_macro_settings": []
    }));
    fs::write(&seed, serde_json::to_vec_pretty(&archive).unwrap()).expect("seed archive");
    run_marginalis(
        &["import-archive", "--input"],
        &seed,
        &database_url,
        "seed identity database",
    );

    let link_backup = directory.join("identity-link.sqlite3");
    let linked = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args([
            "link-identity",
            "--existing-issuer",
            old_issuer,
            "--existing-subject",
            old_subject,
            "--new-issuer",
            new_issuer,
            "--new-subject",
            new_subject,
            "--make-primary",
            "--backup-output",
        ])
        .arg(&link_backup)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("link identity");
    assert!(
        linked.status.success(),
        "identity link failed: {}",
        String::from_utf8_lossy(&linked.stderr)
    );
    assert!(linked.stdout.is_empty());
    let link_log = String::from_utf8(linked.stderr).expect("UTF-8 log");
    assert!(link_log.contains("maintenance.identity_link.completed"));
    for private_value in [
        old_issuer,
        old_subject,
        new_issuer,
        new_subject,
        "principal_id",
    ] {
        assert!(!link_log.contains(private_value));
    }
    assert_eq!(
        fs::metadata(&link_backup)
            .expect("link backup")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let linked_archive = directory.join("linked.json");
    run_marginalis(
        &["export-archive", "--output"],
        &linked_archive,
        &database_url,
        "export linked identities",
    );
    let linked_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&linked_archive).unwrap()).unwrap();
    assert_eq!(linked_json["principals"][1]["primary_issuer"], new_issuer);
    assert_eq!(linked_json["principals"][1]["primary_subject"], new_subject);
    assert_eq!(
        linked_json["principals"][1]["aliases"][0]["issuer"],
        old_issuer
    );
    assert_eq!(
        linked_json["principals"][1]["aliases"][0]["subject"],
        old_subject
    );

    let primary_backup = directory.join("identity-primary.sqlite3");
    let switched = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args([
            "set-primary-identity",
            "--issuer",
            old_issuer,
            "--subject",
            old_subject,
            "--backup-output",
        ])
        .arg(&primary_backup)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("switch primary identity");
    assert!(
        switched.status.success(),
        "primary switch failed: {}",
        String::from_utf8_lossy(&switched.stderr)
    );
    let switch_log = String::from_utf8(switched.stderr).expect("UTF-8 log");
    assert!(switch_log.contains("maintenance.identity_primary.completed"));
    for private_value in [
        old_issuer,
        old_subject,
        new_issuer,
        new_subject,
        "principal_id",
    ] {
        assert!(!switch_log.contains(private_value));
    }
    assert!(primary_backup.is_file());

    let conflict_backup = directory.join("identity-conflict.sqlite3");
    let conflict = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args([
            "link-identity",
            "--existing-issuer",
            old_issuer,
            "--existing-subject",
            old_subject,
            "--new-issuer",
            "https://id.example.test",
            "--new-subject",
            "bob",
            "--backup-output",
        ])
        .arg(&conflict_backup)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("reject conflicting identity");
    assert!(!conflict.status.success());
    assert!(!conflict_backup.exists());
    let conflict_log = String::from_utf8(conflict.stderr).expect("UTF-8 log");
    for private_value in [old_issuer, old_subject, "https://id.example.test", "bob"] {
        assert!(!conflict_log.contains(private_value));
    }

    fs::remove_dir_all(&directory).expect("remove test directory");
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
    assert_eq!(archive_json["format"], "marginalis-archive-18");
    assert_eq!(
        archive_json["adocweave_package_version"],
        marginalis_asciidoc::PINNED_ADOCWEAVE_PACKAGE_VERSION
    );
    assert_eq!(archive_json["note_profile_version"], 6);

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
    let input = directory.join("v0.47.0-archive-17.json");
    let output = directory.join("current-archive-18.json");
    let previous = serde_json::json!({
        "format": "marginalis-archive-17",
        "adocweave_package_version": PREVIOUS_PUBLISHED_ADOCWEAVE_VERSION,
        "note_profile_version": 5,
        "notes": [
            {
                "note_id": "0197c9bc-0000-7000-8000-000000000001",
                "creator_issuer": "https://id.example.test",
                "creator_subject": "alice",
                "source": "= Note\n:source-language: rust\n:marginalis-tags: {source-language}\n\nbody cite:[smith2024]",
                "created_at_ms": 1,
                "updated_at_ms": 2,
                "revision": 2,
                "deleted_at_ms": null,
                "provenance": {
                    "created_via": "web",
                    "review_tracking_known": true,
                    "reviewed_revision": null,
                    "reviewed_at_ms": null,
                    "reviewer_issuer": null,
                    "reviewer_subject": null
                }
            },
            {
                "note_id": "0197c9bc-0000-7000-8000-000000000002",
                "creator_issuer": "https://id.example.test",
                "creator_subject": "alice",
                "source": "= Deleted note\n\nbody",
                "created_at_ms": 1,
                "updated_at_ms": 3,
                "revision": 1,
                "deleted_at_ms": 3,
                "provenance": {
                    "created_via": "mcp",
                    "review_tracking_known": true,
                    "reviewed_revision": null,
                    "reviewed_at_ms": null,
                    "reviewer_issuer": null,
                    "reviewer_subject": null
                }
            }
        ],
        "note_acl": [{
            "note_id": "0197c9bc-0000-7000-8000-000000000001",
            "issuer": "https://id.example.test",
            "subject": "bob",
            "permission": "read"
        }],
        "bibliography_items": [{
            "item_id": "0197c9bc-0000-7000-8000-0000000000a1",
            "owner_issuer": "https://id.example.test",
            "owner_subject": "alice",
            "citation_key": "smith2024",
            "csl_json": { "id": "smith2024", "type": "book", "title": "Example" },
            "created_at_ms": 1,
            "updated_at_ms": 2,
            "revision": 1
        }]
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
    assert_eq!(migrated["format"], "marginalis-archive-18");
    assert_eq!(
        migrated["adocweave_package_version"],
        marginalis_asciidoc::PINNED_ADOCWEAVE_PACKAGE_VERSION
    );
    assert_eq!(migrated["note_profile_version"], 6);
    assert_eq!(migrated["note_revisions"].as_array().map(Vec::len), Some(2));
    assert!(
        migrated["note_revisions"]
            .as_array()
            .expect("note revisions")
            .iter()
            .all(|revision| revision["kind"] == "imported")
    );
    assert_eq!(migrated["math_macro_settings"], serde_json::json!([]));
    assert_eq!(
        migrated["bibliography_import_sources"],
        serde_json::json!([])
    );
    assert_eq!(migrated["bibliography_import_links"], serde_json::json!([]));
    for field in ["note_acl", "bibliography_items"] {
        assert_eq!(migrated[field], previous[field], "changed field: {field}");
    }
    assert_eq!(
        migrated["notes"][0]["source"],
        previous["notes"][0]["source"]
    );
    assert_eq!(
        migrated["notes"][0]["provenance"], previous["notes"][0]["provenance"],
        "provenance must be preserved"
    );

    let verified = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["verify-restore", "--input"])
        .arg(&output)
        .output()
        .expect("verify migrated archive");
    assert!(
        verified.status.success(),
        "restore verification failed: {}",
        String::from_utf8_lossy(&verified.stderr)
    );

    let invalid_input = directory.join("invalid-v0.45.0-archive-17.json");
    let invalid_output = directory.join("invalid-current-archive-16.json");
    let mut invalid = previous;
    invalid["notes"][0]["source"] = concat!(
        "= Note\n:marginalis-tags: research, + \\",
        "\n  rust\n\nbody"
    )
    .into();
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
        .env("MARGINALIS_OIDC_CLIENT_SECRET", "must-not-be-reported")
        .output()
        .expect("diagnose database");
    assert!(healthy.status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&healthy.stdout).expect("diagnostic JSON");
    assert_eq!(report["status"], "ok");
    assert_eq!(report["database"]["schema"]["actual"], 23);
    assert!(!String::from_utf8_lossy(&healthy.stdout).contains("must-not-be-reported"));
    assert!(!String::from_utf8_lossy(&healthy.stderr).contains("must-not-be-reported"));

    fs::remove_dir_all(&directory).expect("remove test directory");
}

/// 空白だけの値を、診断と起動処理が同じように「未設定」と判断することを確認する。
///
/// 以前は診断が`trim`後の空判定、起動処理が`trim`なしの空判定を使っており、空白だけの値に対して
/// 診断は「未設定」、起動処理は「設定済み」と報告が食い違っていた。
#[test]
fn blank_values_are_unset_for_both_diagnose_and_startup() {
    let directory = test_directory("blank-configuration");
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

    let diagnosed = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .arg("diagnose")
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .env("MARGINALIS_OIDC_CLIENT_ID", "   ")
        .output()
        .expect("diagnose database");
    assert!(diagnosed.status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&diagnosed.stdout).expect("diagnostic JSON");
    assert_eq!(
        report["configuration"]["variables"]["MARGINALIS_OIDC_CLIENT_ID"]["set"],
        false
    );

    let started = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .env("MARGINALIS_BASE_URL", "https://notes.example.test")
        .env("MARGINALIS_LISTEN_ADDR", "127.0.0.1:0")
        .env("MARGINALIS_OIDC_ISSUER_URL", "https://id.example.test")
        .env("MARGINALIS_OIDC_CLIENT_ID", "   ")
        .env("MARGINALIS_OIDC_CLIENT_SECRET", "test-only-secret")
        .output()
        .expect("start service");
    assert!(!started.status.success());
    assert!(
        String::from_utf8_lossy(&started.stderr).contains("MARGINALIS_OIDC_CLIENT_ID"),
        "起動処理も未設定として拒否します: {}",
        String::from_utf8_lossy(&started.stderr)
    );

    fs::remove_dir_all(&directory).expect("remove test directory");
}

/// MCPの有効・無効が、内蔵Authorization Server用の明示的なフラグで決まることを確認する。
#[test]
fn mcp_is_enabled_by_the_internal_authorization_server_flag() {
    let directory = test_directory("mcp-enablement");
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

    let disabled = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .arg("diagnose")
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("diagnose without issuer");
    let report: serde_json::Value =
        serde_json::from_slice(&disabled.stdout).expect("diagnostic JSON");
    assert_eq!(report["configuration"]["mcp_enabled"], false);
    assert_eq!(
        report["configuration"]["variables"]["MARGINALIS_MCP_ENABLE"]["set"],
        false
    );

    let enabled = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .arg("diagnose")
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .env("MARGINALIS_MCP_ENABLE", "true")
        .output()
        .expect("diagnose with issuer");
    let report: serde_json::Value =
        serde_json::from_slice(&enabled.stdout).expect("diagnostic JSON");
    assert_eq!(report["configuration"]["mcp_enabled"], true);
    assert_eq!(
        report["configuration"]["variables"]["MARGINALIS_MCP_ENABLE"]["set"],
        true
    );

    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn diagnose_reports_an_unavailable_database_and_fails() {
    let directory = test_directory("unavailable-diagnostics");
    let database_url = format!("sqlite://{}/missing.sqlite3?mode=ro", directory.display());

    let output = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .arg("diagnose")
        .env("MARGINALIS_DATABASE_URL", database_url)
        .env("MARGINALIS_OIDC_CLIENT_SECRET", "must-not-be-reported")
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

#[test]
fn document_export_writes_asciidoc_and_csl_json_with_a_versioned_manifest() {
    let directory = test_directory("document-export");
    fs::create_dir(&directory).expect("test directory");
    let database = directory.join("marginalis.sqlite3");
    let database_url = format!("sqlite://{}?mode=rwc", database.display());
    let archive = directory.join("archive.json");
    let source = current_archive_with_imported_history(serde_json::json!({
        "format": "marginalis-archive-18",
        "adocweave_package_version": marginalis_asciidoc::PINNED_ADOCWEAVE_PACKAGE_VERSION,
        "note_profile_version": 6,
        "principals": [{
            "primary_issuer": "https://id.example.test",
            "primary_subject": "alice",
            "aliases": []
        }, {
            "primary_issuer": "https://id.example.test",
            "primary_subject": "bob",
            "aliases": []
        }],
        "notes": [
            {
                "note_id": "0197c9bc-0000-7000-8000-000000000001",
                "creator_issuer": "https://id.example.test",
                "creator_subject": "alice",
                "source": "= 先行研究の整理\n:marginalis-tags: 研究\n\n本文 cite:[smith2024]",
                "created_at_ms": 1,
                "updated_at_ms": 2,
                "revision": 1,
                "deleted_at_ms": null,
                "provenance": {
                    "created_via": "rest", "review_tracking_known": true,
                    "reviewed_revision": null, "reviewed_at_ms": null,
                    "reviewer_issuer": null, "reviewer_subject": null
                }
            },
            {
                "note_id": "0197c9bc-0000-7000-8000-000000000002",
                "creator_issuer": "https://id.example.test",
                "creator_subject": "alice",
                "source": "= 削除済み\n\n本文",
                "created_at_ms": 1,
                "updated_at_ms": 2,
                "revision": 1,
                "deleted_at_ms": 2,
                "provenance": {
                    "created_via": "rest", "review_tracking_known": true,
                    "reviewed_revision": null, "reviewed_at_ms": null,
                    "reviewer_issuer": null, "reviewer_subject": null
                }
            }
        ],
        "note_acl": [{
            "note_id": "0197c9bc-0000-7000-8000-000000000001",
            "issuer": "https://id.example.test",
            "subject": "bob",
            "permission": "read"
        }],
        "bibliography_items": [{
            "item_id": "0197c9bc-0000-7000-8000-0000000000a1",
            "owner_issuer": "https://id.example.test",
            "owner_subject": "alice",
            "citation_key": "smith2024",
            "csl_json": { "id": "smith2024", "type": "book", "title": "Example" },
            "created_at_ms": 1,
            "updated_at_ms": 2,
            "revision": 1
        }],
        "bibliography_import_sources": [],
        "bibliography_import_links": [],
        "math_macro_settings": []
    }));
    fs::write(
        &archive,
        serde_json::to_vec_pretty(&source).expect("archive JSON"),
    )
    .expect("write archive");
    let imported = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["import-archive", "--input"])
        .arg(&archive)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("import archive");
    assert!(
        imported.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&imported.stderr)
    );

    let output = directory.join("2026-07-31.tar.xz");
    let exported = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["export-documents", "--output"])
        .arg(&output)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("export documents");
    assert!(
        exported.status.success(),
        "document export failed: {}",
        String::from_utf8_lossy(&exported.stderr)
    );
    assert!(exported.stdout.is_empty());
    assert_eq!(
        fs::metadata(&output)
            .expect("archive metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    // 展開して内容を確かめる。書庫の最上位は出力ファイル名から作る。
    let extracted = directory.join("extracted");
    fs::create_dir(&extracted).expect("extract directory");
    let unpacked = Command::new("tar")
        .args(["-xJf"])
        .arg(&output)
        .arg("-C")
        .arg(&extracted)
        .output()
        .expect("extract document export");
    assert!(
        unpacked.status.success(),
        "extract failed: {}",
        String::from_utf8_lossy(&unpacked.stderr)
    );

    let root = extracted.join("2026-07-31");
    let owner = root.join("id.example.test").join("alice");
    let note = owner
        .join("notes")
        .join("先行研究の整理-0197c9bc-0000-7000-8000-000000000001.adoc");
    assert_eq!(
        fs::read_to_string(&note).expect("note file"),
        "= 先行研究の整理\n:marginalis-tags: 研究\n\n本文 cite:[smith2024]"
    );
    // 削除済みのノートは書き出さない。
    assert_eq!(
        fs::read_dir(owner.join("notes"))
            .expect("notes directory")
            .count(),
        1
    );

    let bibliography: serde_json::Value = serde_json::from_slice(
        &fs::read(owner.join("bibliography.json")).expect("bibliography file"),
    )
    .expect("CSL-JSON");
    assert_eq!(bibliography[0]["id"], "smith2024");
    assert_eq!(bibliography[0]["title"], "Example");

    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).expect("manifest"))
            .expect("manifest JSON");
    assert_eq!(manifest["format"], "marginalis-documents-3");
    assert_eq!(manifest["marginalis_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        manifest["adocweave_package_version"],
        marginalis_asciidoc::PINNED_ADOCWEAVE_PACKAGE_VERSION
    );
    assert_eq!(manifest["note_profile_version"], 6);
    assert_eq!(manifest["owners"][0]["subject"], "alice");
    assert_eq!(
        manifest["owners"][0]["notes"][0]["note_id"],
        "0197c9bc-0000-7000-8000-000000000001"
    );
    assert_eq!(
        manifest["owners"][0]["notes"][0]["acl"][0]["subject"],
        "bob"
    );
    let state_sha256 = manifest["owners"][0]["notes"][0]["state_sha256"]
        .as_str()
        .expect("note state SHA-256");
    assert_eq!(state_sha256.len(), 64);
    assert!(state_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(state_sha256, state_sha256.to_ascii_lowercase());
    assert_eq!(
        manifest["owners"][0]["bibliography"][0]["citation_key"],
        "smith2024"
    );

    // 展開しても所有者だけが読める権限になる。
    for path in [&root, &owner] {
        assert_eq!(
            fs::metadata(path)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    for path in [
        &note,
        &owner.join("bibliography.json"),
        &root.join("manifest.json"),
    ] {
        assert_eq!(
            fs::metadata(path)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    // 既存の出力先は上書きしない。
    let repeated = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["export-documents", "--output"])
        .arg(&output)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("repeat document export");
    assert!(!repeated.status.success());

    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn document_import_revalidates_and_restores_into_an_empty_database() {
    let directory = test_directory("document-import");
    fs::create_dir(&directory).expect("test directory");
    let database = directory.join("marginalis.sqlite3");
    let database_url = format!("sqlite://{}?mode=rwc", database.display());
    let archive = directory.join("archive.json");
    let source = current_archive_with_imported_history(serde_json::json!({
        "format": "marginalis-archive-18",
        "adocweave_package_version": marginalis_asciidoc::PINNED_ADOCWEAVE_PACKAGE_VERSION,
        "note_profile_version": 6,
        "principals": [{
            "primary_issuer": "https://id.example.test",
            "primary_subject": "alice",
            "aliases": []
        }, {
            "primary_issuer": "https://id.example.test",
            "primary_subject": "bob",
            "aliases": []
        }],
        // 所有者を2人にし、note IDが所有者をまたいで交互に並ぶようにする。書き出しは所有者ごとに
        // ノートをまとめるため、この並びは取り込み側で読む順とsnapshotの順を食い違わせる。
        "notes": [{
            "note_id": "0197c9bc-0000-7000-8000-000000000001",
            "creator_issuer": "https://id.example.test",
            "creator_subject": "alice",
            "source": "= 先行研究の整理\n:marginalis-tags: 研究\n\n本文",
            "created_at_ms": 1,
            "updated_at_ms": 2,
            "revision": 1,
            "deleted_at_ms": null,
            "provenance": {
                "created_via": "rest", "review_tracking_known": true,
                "reviewed_revision": null, "reviewed_at_ms": null,
                "reviewer_issuer": null, "reviewer_subject": null
            }
        }, {
            "note_id": "0197c9bc-0000-7000-8000-000000000002",
            "creator_issuer": "https://id.example.test",
            "creator_subject": "bob",
            "source": "= 検証メモ\n\n本文",
            "created_at_ms": 1,
            "updated_at_ms": 2,
            "revision": 1,
            "deleted_at_ms": null,
            "provenance": {
                "created_via": "mcp", "review_tracking_known": true,
                "reviewed_revision": null, "reviewed_at_ms": null,
                "reviewer_issuer": null, "reviewer_subject": null
            }
        }, {
            "note_id": "0197c9bc-0000-7000-8000-000000000003",
            "creator_issuer": "https://id.example.test",
            "creator_subject": "alice",
            "source": "= 追加の整理\n\n本文",
            "created_at_ms": 1,
            "updated_at_ms": 2,
            "revision": 1,
            "deleted_at_ms": null,
            "provenance": {
                "created_via": "web", "review_tracking_known": true,
                "reviewed_revision": null, "reviewed_at_ms": null,
                "reviewer_issuer": null, "reviewer_subject": null
            }
        }],
        "note_acl": [{
            "note_id": "0197c9bc-0000-7000-8000-000000000001",
            "issuer": "https://id.example.test",
            "subject": "bob",
            "permission": "edit"
        }],
        // citation_keyの順とitem_idの順をわざと食い違わせる。書き出しはcitation_key順に並べる。
        "bibliography_items": [{
            "item_id": "0197c9bc-0000-7000-8000-0000000000a1",
            "owner_issuer": "https://id.example.test",
            "owner_subject": "alice",
            "citation_key": "tanaka2025",
            "csl_json": { "id": "tanaka2025", "type": "book", "title": "別の文献" },
            "created_at_ms": 1,
            "updated_at_ms": 2,
            "revision": 1
        }, {
            "item_id": "0197c9bc-0000-7000-8000-0000000000a2",
            "owner_issuer": "https://id.example.test",
            "owner_subject": "alice",
            "citation_key": "smith2024",
            "csl_json": { "id": "smith2024", "type": "book", "title": "Example" },
            "created_at_ms": 1,
            "updated_at_ms": 2,
            "revision": 1
        }],
        "bibliography_import_sources": [],
        "bibliography_import_links": [],
        "math_macro_settings": []
    }));
    let archive_bytes = serde_json::to_vec_pretty(&source).expect("archive JSON");
    fs::write(&archive, &archive_bytes).expect("write archive");
    run_marginalis(
        &["import-archive", "--input"],
        &archive,
        &database_url,
        "import archive",
    );

    let documents = directory.join("export.tar.xz");
    run_marginalis(
        &["export-documents", "--output"],
        &documents,
        &database_url,
        "export documents",
    );

    // 別のdatabaseへ取り込み、archiveとして書き出した内容が一致することを確かめる。
    let restored_database = directory.join("restored.sqlite3");
    let restored_url = format!("sqlite://{}?mode=rwc", restored_database.display());
    run_marginalis(
        &["import-documents", "--input"],
        &documents,
        &restored_url,
        "import documents",
    );
    let restored_archive = directory.join("restored.json");
    run_marginalis(
        &["export-archive", "--output"],
        &restored_archive,
        &restored_url,
        "export restored archive",
    );
    let restored: serde_json::Value =
        serde_json::from_slice(&fs::read(&restored_archive).expect("read restored archive"))
            .expect("restored JSON");
    let expected: serde_json::Value =
        serde_json::from_slice(&archive_bytes).expect("source archive JSON");
    assert_eq!(restored, expected);

    // 既に内容があるdatabaseへは取り込まない。
    let repeated = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["import-documents", "--input"])
        .arg(&documents)
        .env("MARGINALIS_DATABASE_URL", &restored_url)
        .output()
        .expect("repeat document import");
    assert!(!repeated.status.success());

    fs::remove_dir_all(&directory).expect("remove test directory");
}

#[test]
fn document_import_rejects_archives_that_escape_their_root() {
    let directory = test_directory("document-import-escape");
    fs::create_dir(&directory).expect("test directory");
    let payload = directory.join("payload");
    fs::create_dir(&payload).expect("payload directory");
    fs::write(payload.join("manifest.json"), b"{}").expect("write manifest");

    let escaping = directory.join("escaping.tar.xz");
    let created = Command::new("tar")
        .args(["-cJf"])
        .arg(&escaping)
        .args(["-C"])
        .arg(&payload)
        .args([
            "--transform",
            "s|manifest.json|../manifest.json|",
            "manifest.json",
        ])
        .output()
        .expect("create escaping archive");
    assert!(
        created.status.success(),
        "archive creation failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );

    let database_url = format!(
        "sqlite://{}?mode=rwc",
        directory.join("marginalis.sqlite3").display()
    );
    let rejected = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["import-documents", "--input"])
        .arg(&escaping)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("reject escaping archive");
    assert!(!rejected.status.success());
    assert!(!directory.join("marginalis.sqlite3").exists());

    fs::remove_dir_all(&directory).expect("remove test directory");
}

fn run_marginalis(command: &[&str], path: &std::path::Path, database_url: &str, purpose: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(command)
        .arg(path)
        .env("MARGINALIS_DATABASE_URL", database_url)
        .output()
        .unwrap_or_else(|error| panic!("{purpose}: {error}"));
    assert!(
        output.status.success(),
        "{purpose} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn restore_archive_migrates_verifies_and_imports_in_one_step() {
    let directory = test_directory("restore-archive");
    fs::create_dir(&directory).expect("test directory");
    let input = directory.join("v0.47.0-archive-17.json");
    let previous = serde_json::json!({
        "format": "marginalis-archive-17",
        "adocweave_package_version": PREVIOUS_PUBLISHED_ADOCWEAVE_VERSION,
        "note_profile_version": 5,
        "notes": [{
            "note_id": "0197c9bc-0000-7000-8000-000000000001",
            "creator_issuer": "https://id.example.test",
            "creator_subject": "alice",
            "source": "= Note\n:marginalis-tags: research\n\nbody",
            "created_at_ms": 1,
            "updated_at_ms": 2,
            "revision": 2,
            "deleted_at_ms": null,
            "provenance": {
                "created_via": "web",
                "review_tracking_known": true,
                "reviewed_revision": null,
                "reviewed_at_ms": null,
                "reviewer_issuer": null,
                "reviewer_subject": null
            }
        }],
        "note_acl": []
    });
    let input_bytes = serde_json::to_vec_pretty(&previous).expect("previous archive");
    fs::write(&input, &input_bytes).expect("write previous archive");
    let database_url = format!(
        "sqlite://{}?mode=rwc",
        directory.join("restored.sqlite").display()
    );

    // 旧契約の書庫を、変換・隔離検証・取り込みまで一括で行う。
    let restored = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["restore-archive", "--input"])
        .arg(&input)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("restore archive");
    assert!(
        restored.status.success(),
        "restore failed: {}",
        String::from_utf8_lossy(&restored.stderr)
    );
    assert!(
        String::from_utf8_lossy(&restored.stderr).contains("maintenance.archive_restore.completed")
    );
    assert_eq!(fs::read(&input).expect("unchanged input"), input_bytes);

    // 取り込み済みdatabaseから現行契約でexportでき、内容が引き継がれている。
    let exported = directory.join("current-archive.json");
    let export = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["export-archive", "--output"])
        .arg(&exported)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("export restored archive");
    assert!(
        export.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    let current: serde_json::Value =
        serde_json::from_slice(&fs::read(&exported).expect("read export")).expect("export JSON");
    assert_eq!(current["format"], "marginalis-archive-18");
    assert_eq!(
        current["adocweave_package_version"],
        marginalis_asciidoc::PINNED_ADOCWEAVE_PACKAGE_VERSION
    );
    assert_eq!(
        current["notes"][0]["source"],
        previous["notes"][0]["source"]
    );
    assert_eq!(current["note_revisions"].as_array().map(Vec::len), Some(1));
    assert_eq!(current["note_revisions"][0]["revision"], 2);
    assert_eq!(current["note_revisions"][0]["kind"], "imported");
    assert_eq!(
        current["note_revisions"][0]["source"],
        previous["notes"][0]["source"]
    );

    // 現行契約の書庫は変換なしで取り込める。
    let second_database_url = format!(
        "sqlite://{}?mode=rwc",
        directory.join("restored-second.sqlite").display()
    );
    let passthrough = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["restore-archive", "--input"])
        .arg(&exported)
        .env("MARGINALIS_DATABASE_URL", &second_database_url)
        .output()
        .expect("restore current archive");
    assert!(
        passthrough.status.success(),
        "passthrough restore failed: {}",
        String::from_utf8_lossy(&passthrough.stderr)
    );

    // 既存データがあるdatabaseへの取り込みは失敗する。
    let repeated = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["restore-archive", "--input"])
        .arg(&input)
        .env("MARGINALIS_DATABASE_URL", &database_url)
        .output()
        .expect("repeat restore");
    assert!(!repeated.status.success());
    assert!(
        String::from_utf8_lossy(&repeated.stderr).contains("maintenance.archive_restore.failed")
    );

    // 対応外の契約は、契約識別子とサポート方針を示して拒否する。
    let unsupported_input = directory.join("unsupported-archive-16.json");
    let mut unsupported = previous;
    unsupported["format"] = "marginalis-archive-16".into();
    unsupported["adocweave_package_version"] = "0.27.0".into();
    fs::write(
        &unsupported_input,
        serde_json::to_vec_pretty(&unsupported).expect("unsupported archive"),
    )
    .expect("write unsupported archive");
    let rejected = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["restore-archive", "--input"])
        .arg(&unsupported_input)
        .env(
            "MARGINALIS_DATABASE_URL",
            format!(
                "sqlite://{}?mode=rwc",
                directory.join("never-created.sqlite").display()
            ),
        )
        .output()
        .expect("reject unsupported archive");
    assert!(!rejected.status.success());
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(stderr.contains("marginalis-archive-16/0.27.0/5"));
    assert!(stderr.contains("直前の公開済み保存契約"));
    assert!(stderr.contains("段階的に変換"));

    let unsupported_output = directory.join("unsupported-current-archive.json");
    let rejected = Command::new(env!("CARGO_BIN_EXE_marginalis-service"))
        .args(["migrate-archive", "--input"])
        .arg(&unsupported_input)
        .arg("--output")
        .arg(&unsupported_output)
        .output()
        .expect("reject unsupported archive migration");
    assert!(!rejected.status.success());
    assert!(!unsupported_output.exists());
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(stderr.contains("marginalis-archive-16/0.27.0/5"));
    assert!(stderr.contains("直前の公開済み保存契約"));
    assert!(stderr.contains("段階的に変換"));

    fs::remove_dir_all(&directory).expect("remove test directory");
}
