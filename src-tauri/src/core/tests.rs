use super::changes::{digest, token};
use super::*;
use super::{configuration::render_profile, domain::now, profiles::load_profile};
use crate::discovery::{BalanceConfig, BalanceSnapshot, DiscoveryResult};
use std::{fs, path::Path};
use toml_edit::DocumentMut;
use uuid::Uuid;
fn profile() -> Profile {
    Profile {
        id: "e7ca67be-ae4f-4ad1-bef2-035b892be342".into(),
        name: "Example".into(),
        base_url: "https://example.test/v1".into(),
        model: "model-test".into(),
        key_stored: true,
        created_at: now(),
        updated_at: now(),
        revision: Uuid::new_v4().to_string(),
        last_validated_at: None,
        models: vec![],
        resolved_base_url: None,
        balance: None,
        balance_config: BalanceConfig::default(),
        last_synced_at: None,
        sync_error: None,
    }
}
#[test]
fn v1_model_migrates_only_when_models_field_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        data: dir.path().join("data"),
        config: dir.path().join("config.toml"),
        helper: dir.path().join("helper"),
    };
    let mut legacy = serde_json::to_value(profile()).unwrap();
    legacy.as_object_mut().unwrap().remove("models");
    atomic_write(
        &paths.profile_file(),
        &serde_json::to_vec(&serde_json::json!({"profiles":[legacy]})).unwrap(),
    )
    .unwrap();
    let migrated = load_store(&paths).unwrap();
    assert_eq!(migrated.profiles[0].models[0].id, "model-test");
    assert!(migrated.profiles[0].models[0].enabled);
    let mut explicit = migrated;
    explicit.profiles[0].models.clear();
    save_store(&paths, &explicit).unwrap();
    assert!(load_store(&paths).unwrap().profiles[0].models.is_empty());
}
#[test]
fn discovery_preserves_manual_selection_and_never_enables_new_models() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        data: dir.path().join("data"),
        config: dir.path().join("config.toml"),
        helper: dir.path().join("helper"),
    };
    let mut saved = profile();
    saved.models = vec![ChannelModel {
        id: "selected".into(),
        alias: "My alias".into(),
        enabled: true,
        reasoning_efforts: None,
        default_reasoning_effort: None,
        native_reasoning: None,
    }];
    save_store(
        &paths,
        &Store {
            profiles: vec![saved.clone()],
            ..Default::default()
        },
    )
    .unwrap();
    let result = DiscoveryResult {
        models_status: "ready".into(),
        models: vec![
            crate::discovery::DiscoveredModel {
                id: "selected".into(),
                name: None,
            },
            crate::discovery::DiscoveredModel {
                id: "new".into(),
                name: None,
            },
        ],
        checked_at: now(),
        ..Default::default()
    };
    let updated = record_discovery(&paths, &saved.id, &saved.revision, result).unwrap();
    assert_eq!(updated.models[0].alias, "My alias");
    assert!(updated.models[0].enabled);
    assert!(!updated.models[1].enabled);
}
#[test]
fn failed_balance_refresh_preserves_last_value_and_disabled_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        data: dir.path().join("data"),
        config: dir.path().join("config.toml"),
        helper: dir.path().join("helper"),
    };
    let mut saved = profile();
    saved.balance = Some(BalanceSnapshot {
        status: "available".into(),
        remaining: Some(12.75),
        unit: "USD".into(),
        source: "custom".into(),
        checked_at: "2026-10-01T01:00:00Z".into(),
        message: None,
    });
    save_store(
        &paths,
        &Store {
            profiles: vec![saved.clone()],
            ..Default::default()
        },
    )
    .unwrap();
    let failed = || DiscoveryResult {
        balance: BalanceSnapshot {
            status: "error".into(),
            checked_at: "2026-10-03T01:00:00Z".into(),
            message: Some("余额请求超时。".into()),
            ..Default::default()
        },
        checked_at: now(),
        ..Default::default()
    };
    let once = record_discovery(&paths, &saved.id, &saved.revision, failed()).unwrap();
    let twice = record_discovery(&paths, &once.id, &once.revision, failed()).unwrap();
    let balance = twice.balance.as_ref().unwrap();
    assert_eq!(balance.status, "stale");
    assert_eq!(balance.remaining, Some(12.75));
    assert_eq!(balance.unit, "USD");
    assert_eq!(balance.source, "custom");
    assert_eq!(balance.checked_at, "2026-10-01T01:00:00Z");
    assert_eq!(balance.message.as_deref(), Some("余额请求超时。"));
    let mut disabled = twice;
    disabled.balance_config.mode = "disabled".into();
    save_store(
        &paths,
        &Store {
            profiles: vec![disabled.clone()],
            ..Default::default()
        },
    )
    .unwrap();
    let cleared = record_discovery(&paths, &disabled.id, &disabled.revision, failed()).unwrap();
    assert!(cleared.balance.is_none());
}
#[test]
fn missing_balance_measurement_is_never_invented_after_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        data: dir.path().join("data"),
        config: dir.path().join("config.toml"),
        helper: dir.path().join("helper"),
    };
    let saved = profile();
    save_store(
        &paths,
        &Store {
            profiles: vec![saved.clone()],
            ..Default::default()
        },
    )
    .unwrap();
    let result = record_discovery(
        &paths,
        &saved.id,
        &saved.revision,
        DiscoveryResult {
            balance: BalanceSnapshot {
                status: "error".into(),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    let balance = result.balance.unwrap();
    assert_eq!(balance.status, "error");
    assert_eq!(balance.remaining, None);
}
#[test]
fn preserves_unrelated_configuration_and_is_idempotent() {
    let original = "# personal settings\nmodel = \"old\"\n[projects.\"C:\\\\code\"]\ntrust_level = \"trusted\"\n[model_providers.existing]\nname = \"Keep me\"\n";
    let output =
        render_profile(original, &profile(), Path::new("C:/Program Files/Vela.exe")).unwrap();
    assert!(output.contains("# personal settings"));
    assert!(output.contains("trust_level = \"trusted\""));
    assert!(output.contains("name = \"Keep me\""));
    assert!(!output.contains("experimental_bearer_token"));
    assert_eq!(
        output,
        render_profile(&output, &profile(), Path::new("C:/Program Files/Vela.exe")).unwrap()
    );
}
#[test]
fn refuses_malformed_configuration_and_ids() {
    assert!(render_profile("[broken", &profile(), Path::new("helper.exe")).is_err());
    assert!(validate_id("../escape").is_err());
    assert!(validate_id(&profile().id).is_ok());
}
#[test]
fn preserves_custom_headers_while_removing_conflicting_authorization() {
    let original = format!("[model_providers.{}]\nhttp_headers = {{ Authorization = \"old-secret\", X-Tenant = \"workspace\" }}\n", provider_id(&profile().id));
    let output = render_profile(&original, &profile(), Path::new("helper.exe")).unwrap();
    assert!(!output.contains("old-secret"));
    assert!(output.contains("X-Tenant = \"workspace\""));
}
#[test]
fn preview_token_detects_profile_and_file_changes() {
    assert_ne!(
        token(Some(b"before"), Some(b"after")),
        token(Some(b"external"), Some(b"after"))
    );
    assert_ne!(
        token(Some(b"before"), Some(b"after")),
        token(Some(b"before"), Some(b"new endpoint"))
    );
    assert_ne!(digest(None), digest(Some(b"")));
}
#[test]
fn atomic_replacement_and_store_roundtrip_use_only_temp_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.toml");
    atomic_write(&path, b"first").unwrap();
    atomic_write(&path, b"second").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"second");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
#[test]
fn active_connection_is_derived_from_actual_configuration() {
    let p = profile();
    assert_eq!(
        active_profile_id(
            &format!("model_provider = {:?}", provider_id(&p.id)),
            &[p.clone()]
        ),
        Some(p.id)
    );
    assert!(active_profile_id("model_provider = \"another\"", &[profile()]).is_none());
}
#[test]
fn selected_profile_remains_effective_and_preserves_other_settings() {
    let original = "profile = \"work\"\nmodel_provider = \"old\"\n[profiles.work]\nmodel_provider = \"override\"\napproval_policy = \"on-request\"\n";
    let output = render_profile(original, &profile(), Path::new("helper.exe")).unwrap();
    let doc = output.parse::<DocumentMut>().unwrap();
    assert_eq!(
        doc["profiles"]["work"]["model_provider"].as_str(),
        Some(provider_id(&profile().id).as_str())
    );
    assert_eq!(
        doc["profiles"]["work"]["approval_policy"].as_str(),
        Some("on-request")
    );
    assert_eq!(active_profile_id(&output, &[profile()]), Some(profile().id));
    assert!(profile_matches_configuration(
        &output,
        &profile(),
        Path::new("helper.exe")
    ));
    let mut changed = profile();
    changed.base_url = "https://changed.test/v1".into();
    assert!(!profile_matches_configuration(
        &output,
        &changed,
        Path::new("helper.exe")
    ));
}
#[cfg(windows)]
#[test]
fn backup_is_encrypted_and_restores_exact_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        data: dir.path().join("data"),
        config: dir.path().join("config.toml"),
        helper: dir.path().join("helper.exe"),
    };
    let original = b"# Preserve exactly\nmodel = \"original\"\n";
    atomic_write(&paths.config, original).unwrap();
    let backup = commit_config(
        &paths,
        read_config(&paths).unwrap(),
        Some(b"model = \"next\"\n"),
        "test",
        "test",
    )
    .unwrap();
    let encrypted = fs::read(paths.backup_file(&backup.id).unwrap()).unwrap();
    assert!(!encrypted
        .windows(original.len())
        .any(|part| part == original));
    let preview = preview_restore(&paths, &backup.id).unwrap();
    restore_backup(&paths, &backup.id, &preview.expected_hash).unwrap();
    assert_eq!(fs::read(&paths.config).unwrap(), original);
}
#[cfg(windows)]
#[test]
fn restore_refuses_stale_preview_and_preserves_external_edits() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        data: dir.path().join("data"),
        config: dir.path().join("config.toml"),
        helper: dir.path().join("helper.exe"),
    };
    atomic_write(&paths.config, b"model = \"original\"\n").unwrap();
    let backup = commit_config(
        &paths,
        read_config(&paths).unwrap(),
        Some(b"model = \"next\"\n"),
        "test",
        "test",
    )
    .unwrap();
    let preview = preview_restore(&paths, &backup.id).unwrap();
    atomic_write(
        &paths.config,
        b"# Changed externally\nmodel = \"external\"\n",
    )
    .unwrap();
    assert!(restore_backup(&paths, &backup.id, &preview.expected_hash).is_err());
    assert_eq!(
        fs::read(&paths.config).unwrap(),
        b"# Changed externally\nmodel = \"external\"\n"
    );
}
#[cfg(windows)]
#[test]
fn restores_the_absence_of_a_configuration_file() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        data: dir.path().join("data"),
        config: dir.path().join("config.toml"),
        helper: dir.path().join("helper.exe"),
    };
    let backup = commit_config(&paths, None, Some(b"model = \"next\"\n"), "test", "test").unwrap();
    assert!(!backup.config_existed);
    let preview = preview_restore(&paths, &backup.id).unwrap();
    restore_backup(&paths, &backup.id, &preview.expected_hash).unwrap();
    assert!(!paths.config.exists());
}
#[test]
fn an_external_lock_prevents_mutations() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        data: dir.path().join("data"),
        config: dir.path().join("config.toml"),
        helper: dir.path().join("helper.exe"),
    };
    let _lock = paths.lock().unwrap();
    assert!(paths.lock().is_err());
}
#[test]
fn validation_of_an_old_revision_cannot_mark_an_edited_connection_as_valid() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        data: dir.path().join("data"),
        config: dir.path().join("config.toml"),
        helper: dir.path().join("helper.exe"),
    };
    let original = profile();
    let mut edited = original.clone();
    // Deliberately keep the same second-resolution timestamp to reproduce the race.
    edited.revision = Uuid::new_v4().to_string();
    edited.base_url = "https://changed.test/v1".into();
    let store = Store {
        profiles: vec![edited.clone()],
        backups: vec![],
        ..Default::default()
    };
    save_store(&paths, &store).unwrap();
    assert!(!mark_validated(&paths, &original.id, &original.revision, true).unwrap());
    assert!(load_profile(&paths, &original.id)
        .unwrap()
        .last_validated_at
        .is_none());
    assert!(mark_validated(&paths, &edited.id, &edited.revision, true).unwrap());
    assert!(load_profile(&paths, &edited.id)
        .unwrap()
        .last_validated_at
        .is_some());
    assert!(mark_validated(&paths, &edited.id, &edited.revision, false).unwrap());
    assert!(load_profile(&paths, &edited.id)
        .unwrap()
        .last_validated_at
        .is_none());
}
#[test]
fn changing_an_existing_endpoint_is_rejected_before_any_credential_write() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        data: dir.path().join("data"),
        config: dir.path().join("config.toml"),
        helper: dir.path().join("helper.exe"),
    };
    let original = profile();
    save_store(
        &paths,
        &Store {
            profiles: vec![original.clone()],
            backups: vec![],
            ..Default::default()
        },
    )
    .unwrap();
    let error = save_profile(
        &paths,
        ProfileInput {
            id: Some(original.id.clone()),
            name: original.name.clone(),
            base_url: "https://another-provider.test/v1".into(),
            model: original.model.clone(),
            api_key: Some("new-provider-test-key".into()),
            models: None,
            balance_config: None,
        },
    )
    .unwrap_err();
    assert!(error.contains("新建连接"));
    let unchanged = load_profile(&paths, &original.id).unwrap();
    assert_eq!(unchanged.base_url, original.base_url);
    assert_eq!(unchanged.revision, original.revision);
}
#[cfg(windows)]
#[test]
fn repair_skips_backups_whose_managed_connection_was_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        data: dir.path().join("data"),
        config: dir.path().join("config.toml"),
        helper: dir.path().join("helper.exe"),
    };
    atomic_write(&paths.config, b"model = \"original\"\n").unwrap();
    let managed = render_profile("", &profile(), &paths.helper).unwrap();
    let original = commit_config(
        &paths,
        read_config(&paths).unwrap(),
        Some(managed.as_bytes()),
        "test",
        "test",
    )
    .unwrap();
    let stale = commit_config(
        &paths,
        read_config(&paths).unwrap(),
        Some(b"[broken"),
        "test",
        "test",
    )
    .unwrap();
    assert!(preview_restore(&paths, &stale.id).is_err());
    assert_eq!(
        preview_repair(&paths).unwrap().backup_id.as_deref(),
        Some(original.id.as_str())
    );
}
