use super::{
    commit_config, config_text, configuration::profile_matches_provider, legacy, load_store,
    read_config, AppPaths, Profile,
};
use std::path::Path;
use toml_edit::{value, DocumentMut, Item};

fn rebind_direct_helper(contents: &str, paths: &AppPaths, profiles: &[Profile]) -> Option<String> {
    let mut document = contents.parse::<DocumentMut>().ok()?;
    let managed: Vec<String> = document
        .get("model_providers")?
        .as_table()?
        .iter()
        .filter_map(|(provider_id, provider)| {
            let profile = profiles
                .iter()
                .find(|profile| provider_id == legacy::direct_provider(&profile.id))?;
            let command = provider.get("auth")?.get("command")?.as_str()?;
            if command == paths.helper.to_string_lossy()
                || !legacy::helper_matches(command, &paths.helper)
                || !profile_matches_provider(provider, profile, Path::new(command))
            {
                return None;
            }
            Some(provider_id.to_owned())
        })
        .collect();
    if managed.is_empty() {
        return None;
    }
    for provider_id in managed {
        document["model_providers"][&provider_id]["auth"]["command"] =
            value(paths.helper.to_string_lossy().as_ref());
        if document["model_providers"][&provider_id]
            .get("name")
            .and_then(Item::as_str)
            .is_some_and(legacy::is_default_provider)
        {
            document["model_providers"][&provider_id]["name"] = value("ahaX");
        }
    }
    Some(document.to_string())
}

pub(crate) fn upgrade_legacy_direct_connection(paths: &AppPaths) -> Result<bool, String> {
    let _lock = paths.lock()?;
    let current = read_config(paths)?;
    let contents = config_text(&current)?;
    let store = load_store(paths)?;
    let Some(proposed) = rebind_direct_helper(contents, paths, &store.profiles) else {
        return Ok(false);
    };
    if !paths.helper.is_file() {
        return Err("新版凭据助手不存在，旧连接配置保持不变。".into());
    }
    commit_config(
        paths,
        current,
        Some(proposed.as_bytes()),
        "升级 ahaX 凭据助手",
        "保留直连服务商、模型和自定义设置，仅更新受管凭据助手位置",
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::super::profile_matches_configuration;
    use super::*;
    use serde_json::json;
    use std::fs;

    fn fixture() -> (tempfile::TempDir, AppPaths, Profile, String) {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: directory.path().join("data"),
            config: directory.path().join("config.toml"),
            helper: directory.path().join("ahax.exe"),
            locations: None,
        };
        let profile: Profile = serde_json::from_value(json!({
            "id":uuid::Uuid::new_v4().to_string(), "name":"My channel", "model":"real-model",
            "baseUrl":"https://example.test/v1", "keyStored":true, "createdAt":"now", "updatedAt":"now"
        })).unwrap();
        let provider = legacy::direct_provider(&profile.id);
        let old_helper = directory.path().join("vela.exe");
        let command = toml_edit::Value::from(old_helper.to_string_lossy().as_ref());
        let config = format!(
            "# Preserve this configuration.\nprofile = \"work\"\nmodel = \"unselected-root\"\nmodel_reasoning_effort = \"high\"\n[profiles.work]\nmodel_provider = \"{provider}\"\nmodel = \"real-model\"\nsandbox_mode = \"read-only\"\n[model_providers.{provider}]\nname = \"My channel\"\nbase_url = \"https://example.test/v1\"\nwire_api = \"responses\"\n[model_providers.{provider}.auth]\ncommand = {command}\nargs = [\"--credential\", \"{}\"]\n",
            profile.id
        );
        (directory, paths, profile, config)
    }

    #[test]
    fn direct_upgrade_changes_only_the_exact_owned_helper() {
        let (_directory, paths, profile, config) = fixture();
        let upgraded = rebind_direct_helper(&config, &paths, &[profile.clone()]).unwrap();
        let mut expected = config.parse::<DocumentMut>().unwrap();
        expected["model_providers"][&legacy::direct_provider(&profile.id)]["auth"]["command"] =
            value(paths.helper.to_string_lossy().as_ref());
        assert_eq!(upgraded, expected.to_string());
        assert!(profile_matches_configuration(
            &upgraded,
            &profile,
            &paths.helper
        ));
        assert!(rebind_direct_helper(&upgraded, &paths, &[profile]).is_none());
    }

    #[test]
    fn foreign_helpers_credentials_and_providers_are_never_rebound() {
        let (directory, paths, profile, config) = fixture();
        let provider = legacy::direct_provider(&profile.id);
        let original = config.parse::<DocumentMut>().unwrap();
        let mut different_helper = original.clone();
        different_helper["model_providers"][&provider]["auth"]["command"] = value(
            directory
                .path()
                .join("other/vela.exe")
                .to_string_lossy()
                .as_ref(),
        );
        let mut different_key = original.clone();
        different_key["model_providers"][&provider]["api_key"] = value("external-key");
        let mut different_provider = original.clone();
        let owned_provider = different_provider["model_providers"]
            .as_table_mut()
            .unwrap()
            .remove(&provider)
            .unwrap();
        different_provider["model_providers"]["thirdparty_api"] = owned_provider;
        different_provider["profiles"]["work"]["model_provider"] = value("thirdparty_api");
        let mut different_endpoint = original;
        different_endpoint["model_providers"][&provider]["base_url"] =
            value("https://other.test/v1");
        for document in [
            different_helper,
            different_key,
            different_provider,
            different_endpoint,
        ] {
            assert!(
                rebind_direct_helper(&document.to_string(), &paths, &[profile.clone()]).is_none()
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn native_direct_upgrade_keeps_an_exact_encrypted_backup_and_is_idempotent() {
        let (_directory, paths, profile, config) = fixture();
        fs::write(&paths.helper, b"isolated helper fixture").unwrap();
        fs::write(&paths.config, config.as_bytes()).unwrap();
        super::super::save_store(
            &paths,
            &super::super::Store {
                profiles: vec![profile],
                ..Default::default()
            },
        )
        .unwrap();
        assert!(upgrade_legacy_direct_connection(&paths).unwrap());
        let store = load_store(&paths).unwrap();
        assert_eq!(store.backups.len(), 1);
        let encrypted = fs::read(paths.backup_file(&store.backups[0].id).unwrap()).unwrap();
        assert_eq!(
            crate::security::unprotect(&encrypted).unwrap(),
            config.as_bytes()
        );
        assert!(!upgrade_legacy_direct_connection(&paths).unwrap());
        assert_eq!(load_store(&paths).unwrap().backups.len(), 1);
    }

    #[test]
    fn native_model_selection_and_inactive_profiles_keep_their_choices() {
        let (_directory, paths, profile, config) = fixture();
        let provider = legacy::direct_provider(&profile.id);
        let original = config.parse::<DocumentMut>().unwrap();
        for active_provider in [provider.as_str(), "thirdparty_api"] {
            let mut selected = original.clone();
            selected["profiles"]["work"]["model_provider"] = value(active_provider);
            selected["profiles"]["work"]["model"] = value("native-selected-model");
            selected["profiles"]["work"]["model_reasoning_effort"] = value("xhigh");
            selected["profiles"]["later"] = Item::Table(toml_edit::Table::new());
            selected["profiles"]["later"]["model_provider"] = value(&provider);
            selected["profiles"]["later"]["model"] = value("another-native-model");
            let mut expected = selected.clone();
            expected["model_providers"][&provider]["auth"]["command"] =
                value(paths.helper.to_string_lossy().as_ref());
            let upgraded =
                rebind_direct_helper(&selected.to_string(), &paths, &[profile.clone()]).unwrap();
            assert_eq!(upgraded, expected.to_string());
            assert!(rebind_direct_helper(&upgraded, &paths, &[profile.clone()]).is_none());
        }
    }
}
