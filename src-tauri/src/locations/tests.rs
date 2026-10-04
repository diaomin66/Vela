use super::*;
use std::path::PathBuf;

fn fixture() -> (tempfile::TempDir, AppPaths) {
    let directory = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        data: directory.path().join("anchor"),
        config: directory.path().join("official").join("config.toml"),
        helper: directory.path().join("unused.exe"),
        locations: None,
    };
    let resolved =
        ResolvedLocations::defaults(&paths.data, &paths.codex_home(), &paths.codex_home());
    (directory, paths.with_locations(resolved))
}
fn put(path: &Path, content: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}
fn path(directory: &Path, name: &str) -> Option<String> {
    Some(directory.join(name).to_string_lossy().into_owned())
}

#[test]
fn preview_and_pending_save_leave_active_locations_and_official_files_untouched() {
    let (directory, paths) = fixture();
    put(&paths.config, b"model='original'\n");
    put(
        &paths.backups_directory().join("first.bin"),
        b"encrypted-backup",
    );
    let preferences = LocationPreferences {
        codex_home: path(directory.path(), "new-official"),
        backups_directory: path(directory.path(), "new-backups"),
        ..Default::default()
    };
    let proposal = preview(&paths, preferences.clone()).unwrap();
    assert!(proposal.can_save);
    assert_eq!(
        proposal
            .changes
            .iter()
            .find(|change| change.key == "backupsDirectory")
            .unwrap()
            .files,
        1
    );
    assert!(!directory.path().join("new-backups").exists());
    let stored = save(&paths, preferences, &proposal.expected_hash).unwrap();
    assert!(stored.requires_restart);
    assert_eq!(
        stored.active.config_path,
        validation::display(&paths.config)
    );
    assert_eq!(paths.backups_directory(), paths.data.join("backups"));
    assert_eq!(fs::read(&paths.config).unwrap(), b"model='original'\n");
    assert!(!directory.path().join("new-official").exists());
}

#[test]
fn home_switch_requires_protection_and_recounts_its_new_files_before_saving() {
    let (directory, paths) = fixture();
    let preferences = LocationPreferences {
        codex_home: path(directory.path(), "next-home"),
        thread_protection_directory: path(directory.path(), "next-protection"),
        ..Default::default()
    };
    let proposal = preview(&paths, preferences.clone()).unwrap();
    assert!(save_with_checkpoint(
        &paths,
        preferences.clone(),
        &proposal.expected_hash,
        || Err("protection-failed".into())
    )
    .is_err());
    assert!(!status(&paths).unwrap().requires_restart);
    let protected = paths
        .thread_protection_directory()
        .join("manifests")
        .join("old-source.bin");
    let report = save_with_checkpoint(&paths, preferences, &proposal.expected_hash, || {
        put(&protected, b"retained-before-switch");
        Ok(())
    })
    .unwrap();
    assert!(report.requires_restart);
    let restarted = activate_pending(paths.clone()).unwrap();
    assert_eq!(
        fs::read(
            restarted
                .thread_protection_directory()
                .join("manifests")
                .join("old-source.bin")
        )
        .unwrap(),
        b"retained-before-switch"
    );
    assert!(protected.exists());
    let preferences = LocationPreferences {
        backups_directory: path(directory.path(), "backups-only"),
        ..read(&restarted).unwrap().active
    };
    let proposal = preview(&restarted, preferences.clone()).unwrap();
    save_with_checkpoint(&restarted, preferences, &proposal.expected_hash, || {
        panic!("backup location does not switch official sources")
    })
    .unwrap();
}

#[test]
fn activation_copies_all_stores_and_preserves_identity_credentials_and_sources() {
    let (directory, paths) = fixture();
    put(&paths.data.join("connections.json"), b"anchored-metadata");
    put(&paths.backups_directory().join("first.bin"), b"backup-data");
    put(
        &paths.evaluations_directory().join("runs").join("one.json"),
        b"evaluation-data",
    );
    put(&paths.exports_directory().join("one.json"), b"export-data");
    put(
        &paths
            .thread_protection_directory()
            .join("manifests")
            .join("one.bin"),
        b"protected-data",
    );
    put(
        &paths.thread_protection_directory().join("catalog.bin"),
        b"thread-settings",
    );
    let database = paths.thread_index_directory().join("inventory.sqlite3");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE proof(value TEXT); INSERT INTO proof VALUES('kept');").unwrap();
    let credentials_before = crate::security::gateway_credential_id(&paths);
    let instance_before = paths.instance_identifier().unwrap();
    let preferences = LocationPreferences {
        backups_directory: path(directory.path(), "backups"),
        evaluations_directory: path(directory.path(), "evaluations"),
        exports_directory: path(directory.path(), "exports"),
        thread_protection_directory: path(directory.path(), "protection"),
        thread_index_directory: path(directory.path(), "index"),
        ..Default::default()
    };
    let proposal = preview(&paths, preferences.clone()).unwrap();
    assert!(proposal.can_save, "{:?}", proposal.errors);
    save(&paths, preferences, &proposal.expected_hash).unwrap();
    let activated = activate_pending(paths.clone()).unwrap();
    assert_eq!(activated.data, paths.data);
    assert_eq!(
        crate::security::gateway_credential_id(&activated),
        credentials_before
    );
    assert_eq!(activated.instance_identifier().unwrap(), instance_before);
    assert_eq!(
        fs::read(activated.data.join("connections.json")).unwrap(),
        b"anchored-metadata"
    );
    for (target, content) in [
        (
            activated.backups_directory().join("first.bin"),
            b"backup-data".as_slice(),
        ),
        (
            activated
                .evaluations_directory()
                .join("runs")
                .join("one.json"),
            b"evaluation-data".as_slice(),
        ),
        (
            activated.exports_directory().join("one.json"),
            b"export-data".as_slice(),
        ),
        (
            activated
                .thread_protection_directory()
                .join("manifests")
                .join("one.bin"),
            b"protected-data".as_slice(),
        ),
    ] {
        assert_eq!(fs::read(target).unwrap(), content);
    }
    let copied =
        rusqlite::Connection::open(activated.thread_index_directory().join("inventory.sqlite3"))
            .unwrap();
    assert_eq!(
        copied
            .query_row("SELECT value FROM proof", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "kept"
    );
    assert!(!activated
        .thread_protection_directory()
        .join("inventory.sqlite3")
        .exists());
    assert!(!activated
        .thread_index_directory()
        .join("manifests")
        .exists());
    assert!(database.exists());
    assert!(paths.backups_directory().join("first.bin").exists());
    assert!(!status(&activated).unwrap().requires_restart);
    drop(copied);
    drop(connection);
}

#[cfg(windows)]
#[test]
fn windows_migration_publishes_real_manifest_layout_beyond_max_path() {
    let (directory, paths) = fixture();
    let source_id = "a".repeat(64);
    let thread_key = "b".repeat(64);
    let fingerprint = "c".repeat(64);
    let metadata_hash = "d".repeat(64);
    let relative = PathBuf::from("manifests")
        .join(&source_id)
        .join(thread_key)
        .join(format!("{fingerprint}-{metadata_hash}.bin"));
    let original = paths.thread_protection_directory().join(&relative);
    put(&original, b"complete-encrypted-manifest");
    let object = PathBuf::from("objects")
        .join(source_id)
        .join("cc")
        .join(format!("{fingerprint}.bin"));
    put(
        &paths.thread_protection_directory().join(&object),
        b"complete-encrypted-chunk",
    );
    put(
        &paths.thread_protection_directory().join("catalog.bin"),
        b"source-metadata",
    );
    let target = directory
        .path()
        .join("custom locations")
        .join("thread protection");
    assert!(original.as_os_str().len() > 260);
    assert!(target.join(&relative).as_os_str().len() > 260);
    let preferences = LocationPreferences {
        thread_protection_directory: Some(target.to_string_lossy().into_owned()),
        ..Default::default()
    };
    let proposal = preview(&paths, preferences.clone()).unwrap();
    assert!(proposal.can_save, "{:?}", proposal.errors);
    save(&paths, preferences, &proposal.expected_hash).unwrap();
    let activated = activate_pending(paths.clone()).unwrap();
    assert_eq!(
        fs::read(activated.thread_protection_directory().join(relative)).unwrap(),
        b"complete-encrypted-manifest"
    );
    assert_eq!(
        fs::read(activated.thread_protection_directory().join(object)).unwrap(),
        b"complete-encrypted-chunk"
    );
    assert_eq!(fs::read(original).unwrap(), b"complete-encrypted-manifest");
    assert!(!status(&activated).unwrap().requires_restart);
    assert!(!migration::in_progress(&activated).unwrap());
}

#[test]
fn interrupted_copy_is_idempotent_and_commits_only_after_a_verified_restart() {
    let (directory, paths) = fixture();
    put(&paths.backups_directory().join("one.bin"), b"unchanged");
    let preferences = LocationPreferences {
        backups_directory: path(directory.path(), "backups-next"),
        ..Default::default()
    };
    let proposal = preview(&paths, preferences.clone()).unwrap();
    save(&paths, preferences.clone(), &proposal.expected_hash).unwrap();
    let current = active(&paths);
    let next = resolve::preferences(&paths, &preferences, &current.defaults).unwrap();
    migration::execute(&paths, &current, &next).unwrap();
    assert!(migration::in_progress(&paths).unwrap());
    assert!(status(&paths).unwrap().requires_restart);
    let restarted = activate_pending(paths.clone()).unwrap();
    assert_eq!(
        fs::read(restarted.backups_directory().join("one.bin")).unwrap(),
        b"unchanged"
    );
    assert!(!migration::in_progress(&restarted).unwrap());
    assert!(paths.backups_directory().join("one.bin").exists());
}

#[test]
fn conflicting_targets_never_replace_user_files_or_activate_pending_locations() {
    let (directory, paths) = fixture();
    put(&paths.backups_directory().join("one.bin"), b"original");
    let preferences = LocationPreferences {
        backups_directory: path(directory.path(), "target"),
        ..Default::default()
    };
    let proposal = preview(&paths, preferences.clone()).unwrap();
    save(&paths, preferences, &proposal.expected_hash).unwrap();
    put(
        &directory.path().join("target").join("one.bin"),
        b"unrelated-user-content",
    );
    assert!(activate_pending(paths.clone()).is_err());
    assert_eq!(
        fs::read(directory.path().join("target").join("one.bin")).unwrap(),
        b"unrelated-user-content"
    );
    assert_eq!(
        fs::read(paths.backups_directory().join("one.bin")).unwrap(),
        b"original"
    );
    let report = status(&paths).unwrap();
    assert!(report.requires_restart);
    assert!(report.error.is_some());
}

#[test]
fn directory_overlaps_parent_traversal_and_stale_previews_are_rejected() {
    let (directory, paths) = fixture();
    for candidate in [
        paths.data.clone(),
        paths.codex_home(),
        paths.data.join("evaluations").join("nested"),
    ] {
        let proposal = preview(
            &paths,
            LocationPreferences {
                backups_directory: Some(candidate.to_string_lossy().into_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!proposal.can_save);
    }
    assert!(preview(
        &paths,
        LocationPreferences {
            backups_directory: Some("relative".into()),
            ..Default::default()
        }
    )
    .is_err());
    assert!(preview(
        &paths,
        LocationPreferences {
            backups_directory: path(directory.path(), "child/../escape"),
            ..Default::default()
        }
    )
    .is_err());
    let first = LocationPreferences {
        backups_directory: path(directory.path(), "first"),
        ..Default::default()
    };
    let proposal = preview(&paths, first.clone()).unwrap();
    assert!(save(&paths, first.clone(), "stale-token").is_err());
    save(&paths, first.clone(), &proposal.expected_hash).unwrap();
    assert!(save(&paths, first, &proposal.expected_hash).is_err());
}

#[test]
fn explicit_environment_scope_overrides_preferences_without_accessing_real_environment() {
    let (directory, paths) = fixture();
    let mut defaults = active(&paths).defaults;
    defaults.environment_home = Some(directory.path().join("environment-home"));
    defaults.environment_sqlite = Some(directory.path().join("environment-sqlite"));
    let preferences = LocationPreferences {
        codex_home: path(directory.path(), "user-choice"),
        sqlite_home: path(directory.path(), "user-sqlite"),
        ..Default::default()
    };
    let resolved = resolve::preferences(&paths, &preferences, &defaults).unwrap();
    assert_eq!(
        resolved.codex_home,
        directory.path().join("environment-home")
    );
    assert_eq!(
        resolved.sqlite_home,
        directory.path().join("environment-sqlite")
    );
    assert_eq!(resolve::overrides(&defaults).len(), 2);
    assert!(!resolved.codex_home.exists());
    assert!(!resolved.sqlite_home.exists());
}

#[test]
fn sqlite_default_resolves_official_config_relative_to_the_selected_home() {
    let (directory, paths) = fixture();
    put(&paths.config, b"sqlite_home = 'state'\n");
    let defaults = active(&paths).defaults;
    let resolved =
        resolve::preferences(&paths, &LocationPreferences::default(), &defaults).unwrap();
    assert_eq!(resolved.sqlite_home, paths.codex_home().join("state"));
    let other = directory.path().join("other-home");
    put(&other.join("config.toml"), b"sqlite_home = 'other-state'\n");
    let resolved = resolve::preferences(
        &paths,
        &LocationPreferences {
            codex_home: Some(other.to_string_lossy().into_owned()),
            ..Default::default()
        },
        &defaults,
    )
    .unwrap();
    assert_eq!(resolved.sqlite_home, other.join("other-state"));
    assert_eq!(
        resolved.codex_home.join("config.toml").file_name().unwrap(),
        "config.toml"
    );
}

#[test]
fn default_exports_and_thread_index_follow_their_selected_parent_stores() {
    let (directory, paths) = fixture();
    let preferences = LocationPreferences {
        evaluations_directory: path(directory.path(), "custom-evaluation"),
        thread_protection_directory: path(directory.path(), "custom-protection"),
        ..Default::default()
    };
    let proposal = preview(&paths, preferences).unwrap();
    assert!(proposal.can_save, "{:?}", proposal.errors);
    assert_eq!(
        PathBuf::from(proposal.resolved.exports_directory),
        directory.path().join("custom-evaluation").join("exports")
    );
    assert_eq!(
        proposal.resolved.thread_index_directory,
        proposal.resolved.thread_protection_directory
    );
}

#[test]
fn failed_migration_can_be_cancelled_or_replaced_without_removing_any_copy() {
    let (directory, paths) = fixture();
    put(&paths.backups_directory().join("one.bin"), b"original");
    let preferences = LocationPreferences {
        backups_directory: path(directory.path(), "first-target"),
        ..Default::default()
    };
    let proposal = preview(&paths, preferences.clone()).unwrap();
    save(&paths, preferences.clone(), &proposal.expected_hash).unwrap();
    let current = active(&paths);
    let next = resolve::preferences(&paths, &preferences, &current.defaults).unwrap();
    migration::execute(&paths, &current, &next).unwrap();
    // A failed startup may leave the old active store writable until the next restart.
    put(&paths.backups_directory().join("one.bin"), b"newer-source");
    assert!(activate_pending(paths.clone()).is_err());
    let cancel = preview(&paths, LocationPreferences::default()).unwrap();
    assert!(cancel.can_save);
    assert!(!cancel.requires_restart);
    let report = save(
        &paths,
        LocationPreferences::default(),
        &cancel.expected_hash,
    )
    .unwrap();
    assert!(!report.requires_restart);
    assert!(!migration::in_progress(&paths).unwrap());
    assert_eq!(
        fs::read(next.backups_directory.join("one.bin")).unwrap(),
        b"original"
    );
    assert_eq!(
        fs::read(paths.backups_directory().join("one.bin")).unwrap(),
        b"newer-source"
    );
    assert!(fs::read_dir(&paths.data).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("location-migration-abandoned-")));
    let replacement = LocationPreferences {
        backups_directory: path(directory.path(), "second-target"),
        ..Default::default()
    };
    let proposal = preview(&paths, replacement.clone()).unwrap();
    save(&paths, replacement, &proposal.expected_hash).unwrap();
    let restarted = activate_pending(paths).unwrap();
    assert_eq!(
        fs::read(restarted.backups_directory().join("one.bin")).unwrap(),
        b"newer-source"
    );
}

#[test]
fn disconnected_source_never_activates_an_empty_destination() {
    let (directory, paths) = fixture();
    put(&paths.backups_directory().join("one.bin"), b"original");
    let preferences = LocationPreferences {
        backups_directory: path(directory.path(), "new-target"),
        ..Default::default()
    };
    let proposal = preview(&paths, preferences.clone()).unwrap();
    save(&paths, preferences, &proposal.expected_hash).unwrap();
    fs::rename(
        paths.backups_directory(),
        directory.path().join("disconnected-source"),
    )
    .unwrap();
    assert!(activate_pending(paths.clone()).is_err());
    assert!(status(&paths).unwrap().requires_restart);
    assert!(!directory.path().join("new-target").exists());
    assert_eq!(
        fs::read(directory.path().join("disconnected-source").join("one.bin")).unwrap(),
        b"original"
    );
}

#[test]
fn committed_migration_journal_is_cleaned_after_restart_without_recopying() {
    let (directory, paths) = fixture();
    put(&paths.backups_directory().join("one.bin"), b"original");
    let preferences = LocationPreferences {
        backups_directory: path(directory.path(), "new-target"),
        ..Default::default()
    };
    let proposal = preview(&paths, preferences.clone()).unwrap();
    save(&paths, preferences.clone(), &proposal.expected_hash).unwrap();
    let current = active(&paths);
    let next = resolve::preferences(&paths, &preferences, &current.defaults).unwrap();
    migration::execute(&paths, &current, &next).unwrap();
    let mut stored = read(&paths).unwrap();
    stored.active = preferences;
    stored.pending = None;
    stored.pending_resolved = None;
    stored.pending_sources.clear();
    write(&paths, &stored).unwrap();
    let restarted = activate_pending(paths.with_locations(next)).unwrap();
    assert!(!migration::in_progress(&restarted).unwrap());
    assert!(!status(&restarted).unwrap().requires_restart);
    assert_eq!(
        fs::read(restarted.backups_directory().join("one.bin")).unwrap(),
        b"original"
    );
}

#[test]
fn malformed_official_config_keeps_diagnostics_available_and_exposes_scope_error() {
    let (_directory, paths) = fixture();
    for contents in [
        b"sqlite_home = 17\n".as_slice(),
        b"invalid [",
        b"\xff\xfe\x00",
    ] {
        put(&paths.config, contents);
        let resolved = resolve::preferences(
            &paths,
            &LocationPreferences::default(),
            &active(&paths).defaults,
        )
        .unwrap();
        assert_eq!(resolved.sqlite_home, paths.codex_home());
        assert!(resolved.error.is_some());
        let paths = paths.clone().with_locations(resolved);
        assert!(status(&paths).unwrap().error.is_some());
        assert!(paths.location_error().is_some());
        assert!(preview(&paths, LocationPreferences::default()).is_ok());
    }
}

#[test]
fn identity_metadata_subdirectories_are_not_allowed_as_custom_stores() {
    let (_directory, paths) = fixture();
    let proposal = preview(
        &paths,
        LocationPreferences {
            backups_directory: Some(paths.data.join("catalogs").to_string_lossy().into_owned()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!proposal.can_save);
}

#[test]
fn split_exports_and_inventory_can_return_to_the_default_shared_directories() {
    let (directory, paths) = fixture();
    put(
        &paths.evaluations_directory().join("runs").join("one.json"),
        b"run",
    );
    put(
        &paths.thread_protection_directory().join("catalog.bin"),
        b"catalog",
    );
    let preferences = LocationPreferences {
        exports_directory: path(directory.path(), "split-exports"),
        thread_index_directory: path(directory.path(), "split-index"),
        ..Default::default()
    };
    let proposal = preview(&paths, preferences.clone()).unwrap();
    save(&paths, preferences, &proposal.expected_hash).unwrap();
    let split = activate_pending(paths).unwrap();
    put(&split.exports_directory().join("new.json"), b"export");
    fs::create_dir_all(split.thread_index_directory()).unwrap();
    let connection =
        rusqlite::Connection::open(split.thread_index_directory().join("inventory.sqlite3"))
            .unwrap();
    connection
        .execute_batch("CREATE TABLE proof(value TEXT); INSERT INTO proof VALUES('split');")
        .unwrap();
    drop(connection);
    let proposal = preview(&split, LocationPreferences::default()).unwrap();
    assert!(proposal.can_save, "{:?}", proposal.errors);
    save(
        &split,
        LocationPreferences::default(),
        &proposal.expected_hash,
    )
    .unwrap();
    let merged = activate_pending(split).unwrap();
    assert_eq!(
        merged.thread_index_directory(),
        merged.thread_protection_directory()
    );
    assert_eq!(
        fs::read(merged.exports_directory().join("new.json")).unwrap(),
        b"export"
    );
    assert_eq!(
        fs::read(merged.thread_protection_directory().join("catalog.bin")).unwrap(),
        b"catalog"
    );
    assert_eq!(
        fs::read(merged.evaluations_directory().join("runs").join("one.json")).unwrap(),
        b"run"
    );
}

#[cfg(windows)]
#[test]
fn directory_junctions_are_rejected_before_reading_or_copying_their_contents() {
    use std::os::windows::process::CommandExt;
    let (directory, paths) = fixture();
    let outside = directory.path().join("outside");
    put(&outside.join("private.bin"), b"must-not-copy");
    let junction = directory.path().join("junction");
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let proposal = preview(
        &paths,
        LocationPreferences {
            backups_directory: Some(junction.to_string_lossy().into_owned()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!proposal.can_save);
    assert_eq!(
        fs::read(outside.join("private.bin")).unwrap(),
        b"must-not-copy"
    );
}
