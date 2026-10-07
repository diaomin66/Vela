use crate::core::{AppPaths, Store};
use std::path::Path;
use toml_edit::{DocumentMut, Item};

fn same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy().replace('/', "\\").to_lowercase()
            == right.to_string_lossy().replace('/', "\\").to_lowercase()
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

pub(crate) fn owns_model_catalog(paths: &AppPaths, value: &str) -> bool {
    let candidate = Path::new(value);
    if !candidate.is_absolute()
        || candidate
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        || !candidate.parent().is_some_and(|parent| {
            same_path(parent, &paths.data.join("catalogs"))
                || crate::core::legacy::catalog_roots(paths)
                    .iter()
                    .any(|root| same_path(parent, &root.join("catalogs")))
        })
    {
        return false;
    }
    candidate
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("models-")?.strip_suffix(".json"))
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|value| value.is_ascii_hexdigit()))
}

fn selected(doc: &DocumentMut) -> Option<&Item> {
    doc.get("profile")
        .and_then(Item::as_str)
        .and_then(|name| doc.get("profiles")?.get(name))
}

fn effective<'a>(doc: &'a DocumentMut, key: &str) -> Option<&'a Item> {
    selected(doc)
        .and_then(|profile| profile.get(key))
        .or_else(|| doc.get(key))
}

pub(crate) fn clear_owned_catalogs(doc: &mut DocumentMut, paths: &AppPaths) {
    if doc
        .get("model_catalog_json")
        .and_then(Item::as_str)
        .is_some_and(|value| owns_model_catalog(paths, value))
    {
        doc.remove("model_catalog_json");
    }
    let name = doc.get("profile").and_then(Item::as_str).map(str::to_owned);
    if let Some(table) = name
        .as_deref()
        .and_then(|name| doc.get_mut("profiles")?.get_mut(name)?.as_table_mut())
    {
        if table
            .get("model_catalog_json")
            .and_then(Item::as_str)
            .is_some_and(|value| owns_model_catalog(paths, value))
        {
            table.remove("model_catalog_json");
        }
    }
}

pub(super) fn managed_provider(paths: &AppPaths, provider: &Item) -> bool {
    let Some(url) = provider
        .get("base_url")
        .and_then(Item::as_str)
        .and_then(|value| url::Url::parse(value).ok())
    else {
        return false;
    };
    let auth = provider.get("auth");
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && matches!(url.path().trim_end_matches('/'), "" | "/v1")
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && provider.get("wire_api").and_then(Item::as_str) == Some("responses")
        && auth
            .and_then(|auth| auth.get("command"))
            .and_then(Item::as_str)
            .is_some_and(|command| crate::core::legacy::helper_matches(command, &paths.helper))
        && auth
            .and_then(|auth| auth.get("args"))
            .and_then(Item::as_array)
            .is_some_and(|args| {
                args.len() == 1
                    && args.get(0).and_then(toml_edit::Value::as_str)
                        == Some("--gateway-credential")
            })
}

fn effective_gateway_provider(paths: &AppPaths, doc: &DocumentMut) -> bool {
    effective(doc, "model_provider")
        .and_then(Item::as_str)
        .and_then(|name| doc.get("model_providers")?.get(name))
        .is_some_and(|provider| managed_provider(paths, provider))
}

pub(crate) fn upgrade_legacy_connection(paths: &AppPaths) -> Result<bool, String> {
    let _lock = paths.lock()?;
    let mut store = crate::core::load_store(paths)?;
    let current = crate::core::read_config(paths)?;
    let contents = crate::core::config_text(&current)?;
    let Ok(doc) = contents.parse::<DocumentMut>() else {
        return Ok(false);
    };
    let mut rebound = doc.clone();
    let helper_changed = rebind_legacy_helpers(paths, &mut rebound);
    let scope_upgrade = needs_scope_upgrade(paths, &store, &doc);
    if !helper_changed && !scope_upgrade {
        return Ok(false);
    }
    if !paths.helper.is_file() {
        return Err("升级后的凭据程序不存在，请重新安装 ahaX。".into());
    }
    let mut migration_error = None;
    let proposed = if scope_upgrade {
        match upgraded_configuration(paths, &store, &rebound) {
            Ok(proposed) => proposed,
            Err(error) if helper_changed => {
                migration_error = Some(error);
                rebound.to_string()
            }
            Err(error) => return Err(error),
        }
    } else {
        rebound.to_string()
    };
    if proposed == contents {
        return Ok(false);
    }
    crate::core::commit_config(
        paths,
        current,
        Some(proposed.as_bytes()),
        "升级本机连接",
        "保留历史服务商并更新 ahaX 凭据程序",
    )?;
    store = crate::core::load_store(paths)?;
    store.default_route_id = store
        .default_route_id
        .as_deref()
        .map(super::canonical_route_id);
    crate::core::save_store(paths, &store)?;
    match migration_error {
        Some(error) => Err(format!(
            "历史连接的凭据程序已更新，但模型目录升级尚未完成：{error}"
        )),
        None => Ok(true),
    }
}

fn rebind_legacy_helpers(paths: &AppPaths, doc: &mut DocumentMut) -> bool {
    let managed = doc
        .get("model_providers")
        .and_then(Item::as_table)
        .map(|providers| {
            providers
                .iter()
                .filter(|(_, provider)| managed_provider(paths, provider))
                .filter(|(_, provider)| {
                    provider
                        .get("auth")
                        .and_then(|auth| auth.get("command"))
                        .and_then(Item::as_str)
                        .is_some_and(|command| !same_path(Path::new(command), &paths.helper))
                })
                .map(|(name, _)| name.to_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for name in &managed {
        doc["model_providers"][name]["auth"]["command"] =
            toml_edit::value(paths.helper.to_string_lossy().as_ref());
    }
    !managed.is_empty()
}

fn needs_scope_upgrade(paths: &AppPaths, store: &Store, doc: &DocumentMut) -> bool {
    let Some(provider_name) = effective(&doc, "model_provider").and_then(Item::as_str) else {
        return false;
    };
    let Some(provider) = doc
        .get("model_providers")
        .and_then(|providers| providers.get(provider_name))
    else {
        return false;
    };
    if !managed_provider(paths, provider)
        || !effective(&doc, "model_catalog_json")
            .and_then(Item::as_str)
            .is_some_and(|path| owns_model_catalog(paths, path))
    {
        return false;
    }
    let Some(route) = configured_route(store, &doc.to_string()) else {
        return false;
    };
    let legacy_model = effective(&doc, "model")
        .and_then(Item::as_str)
        .is_some_and(|model| model != route);
    let legacy_helper = provider
        .get("auth")
        .and_then(|auth| auth.get("command"))
        .and_then(Item::as_str)
        .is_some_and(|command| !same_path(Path::new(command), &paths.helper));
    let legacy_catalog = effective(&doc, "model_catalog_json")
        .and_then(Item::as_str)
        .is_some_and(|path| {
            Path::new(path)
                .parent()
                .is_some_and(|parent| !same_path(parent, &paths.data.join("catalogs")))
        });
    let legacy_provider = provider_name == "Vela" && store.settings.provider_name == "ahaX";
    legacy_model || legacy_helper || legacy_catalog || legacy_provider
}

fn upgraded_configuration(
    paths: &AppPaths,
    store: &Store,
    current: &DocumentMut,
) -> Result<String, String> {
    let mut proposed = current.clone();
    rebind_legacy_helpers(paths, &mut proposed);
    let rename_provider = store.settings.provider_name == "ahaX"
        && effective(current, "model_provider").and_then(Item::as_str) == Some("Vela")
        && current
            .get("model_providers")
            .and_then(|providers| providers.get("Vela"))
            .is_some_and(|provider| managed_provider(paths, provider));
    if rename_provider {
        proposed["model_providers"]["Vela"]["name"] = toml_edit::value("ahaX");
        if let Some(existing) = proposed
            .get("model_providers")
            .and_then(|providers| providers.get("ahaX"))
        {
            if !super::equivalent_item(existing, &proposed["model_providers"]["Vela"]) {
                return Err("ahaX 服务商名称已被其他配置使用，请手动预览并应用连接。".into());
            }
        } else {
            proposed["model_providers"]["ahaX"] = proposed["model_providers"]["Vela"].clone();
        }
    }
    let migrate_scope = |scope: &mut toml_edit::Table| -> Result<(), String> {
        if rename_provider && scope.get("model_provider").and_then(Item::as_str) == Some("Vela") {
            scope["model_provider"] = toml_edit::value("ahaX");
        }
        if let Some(model) = scope.get("model").and_then(Item::as_str) {
            if super::is_internal_route_id(model) {
                scope["model"] = toml_edit::value(super::canonical_route_id(model));
            }
        }
        if let Some(path) = scope.get("model_catalog_json").and_then(Item::as_str) {
            if owns_model_catalog(paths, path) {
                let migrated = migrate_catalog(paths, path)?;
                scope["model_catalog_json"] = toml_edit::value(migrated.to_string_lossy().as_ref());
            }
        }
        Ok(())
    };
    migrate_scope(proposed.as_table_mut())?;
    if let Some(name) = current.get("profile").and_then(Item::as_str) {
        if let Some(scope) = proposed
            .get_mut("profiles")
            .and_then(|profiles| profiles.get_mut(name))
            .and_then(Item::as_table_mut)
        {
            migrate_scope(scope)?;
        }
    }
    Ok(proposed.to_string())
}

fn migrate_catalog(paths: &AppPaths, path: &str) -> Result<std::path::PathBuf, String> {
    use sha2::{Digest, Sha256};
    crate::locations::guard_brand_path(Path::new(path), false)?;
    let metadata = std::fs::metadata(path).map_err(|_| "历史模型目录已丢失，请重新应用连接。")?;
    if metadata.len() > 64 * 1024 * 1024 {
        return Err("历史模型目录过大，未修改连接。".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "无法读取历史模型目录。")?;
    let mut catalog: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "历史模型目录损坏，未修改连接。")?;
    let models = catalog
        .get_mut("models")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or("历史模型目录格式无效，未修改连接。")?;
    let mut identities = std::collections::HashSet::new();
    for model in models {
        let slug = model
            .get("slug")
            .and_then(serde_json::Value::as_str)
            .ok_or("历史模型缺少标识。")?;
        if !super::is_internal_route_id(slug) || !identities.insert(super::canonical_route_id(slug))
        {
            return Err("历史模型目录含非受管或重复模型，未修改连接。".into());
        }
        model["slug"] = serde_json::Value::String(super::canonical_route_id(slug));
    }
    let data = serde_json::to_vec(&catalog).map_err(|_| "无法升级模型目录。")?;
    let target = paths
        .data
        .join("catalogs")
        .join(format!("models-{:x}.json", Sha256::digest(&data)));
    crate::core::atomic_write(
        &target,
        &serde_json::to_vec_pretty(&catalog).map_err(|_| "无法升级模型目录。")?,
    )?;
    Ok(target)
}

pub(crate) fn configured_route(store: &Store, contents: &str) -> Option<String> {
    let doc = contents.parse::<DocumentMut>().ok()?;
    let model = effective(&doc, "model").and_then(Item::as_str)?;
    super::entries(&store.profiles)
        .into_iter()
        .find(|entry| super::route_ids_match(&entry.route_id, model) && entry.enabled)
        .map(|entry| entry.route_id)
}

pub(crate) fn routing_mismatch(paths: &AppPaths, store: &Store, contents: &str) -> bool {
    let Ok(doc) = contents.parse::<DocumentMut>() else {
        return false;
    };
    let known_route = effective(&doc, "model")
        .and_then(Item::as_str)
        .is_some_and(|model| {
            super::is_internal_route_id(model)
                || super::entries(&store.profiles)
                    .iter()
                    .any(|entry| super::route_ids_match(&entry.route_id, model))
        });
    let owned_catalog = effective(&doc, "model_catalog_json")
        .and_then(Item::as_str)
        .is_some_and(|value| owns_model_catalog(paths, value));
    (known_route || owned_catalog) && !effective_gateway_provider(paths, &doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use toml_edit::value;

    fn fixture() -> (tempfile::TempDir, AppPaths, Store) {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: directory.path().join("data"),
            config: directory.path().join("config.toml"),
            helper: directory.path().join("ahax.exe"),
            locations: None,
        };
        let store = serde_json::from_value(json!({"profiles":[{"id":"e7ca67be-ae4f-4ad1-bef2-035b892be342","name":"Fixture","baseUrl":"https://example.test/v1","model":"upstream","keyStored":false,"createdAt":"now","updatedAt":"now","models":[{"id":"upstream","enabled":true}]}]})).unwrap();
        (directory, paths, store)
    }

    #[test]
    fn ownership_requires_this_data_directory_and_exact_generated_filename() {
        let (_directory, paths, store) = fixture();
        let owned = super::super::model_file(&paths, &store);
        assert!(owns_model_catalog(&paths, &owned.to_string_lossy()));
        for candidate in [
            paths.data.join(owned.file_name().unwrap()),
            paths.data.join("catalogs/thirdparty-models.json"),
            paths.data.join("catalogs/models-a.json"),
            paths
                .data
                .join("other/catalogs")
                .join(owned.file_name().unwrap()),
            paths
                .data
                .join("catalogs/../catalogs")
                .join(owned.file_name().unwrap()),
        ] {
            assert!(!owns_model_catalog(&paths, &candidate.to_string_lossy()));
        }
        assert!(!owns_model_catalog(&paths, "models-vela.json"));
        assert!(
            !paths.data.exists(),
            "Ownership checking must not touch files or credentials"
        );
    }

    #[test]
    fn only_known_routes_or_owned_catalogs_can_trigger_a_mismatch() {
        let (_directory, paths, store) = fixture();
        let route = super::super::entries(&store.profiles)[0].route_id.clone();
        let normal = super::super::render("", &paths, &store, &route).unwrap();
        assert!(!routing_mismatch(&paths, &store, &normal));
        let mut doc = normal.parse::<DocumentMut>().unwrap();
        doc["model_provider"] = value("thirdparty");
        assert!(routing_mismatch(&paths, &store, &doc.to_string()));
        doc["model"] = value("upstream");
        assert!(
            routing_mismatch(&paths, &store, &doc.to_string()),
            "Owned picker routes still need the local gateway"
        );
        doc["model_catalog_json"] = value("thirdparty-models.json");
        assert!(!routing_mismatch(&paths, &store, &doc.to_string()));
        doc["model"] = value(format!("vela-{}", "f".repeat(64)));
        assert!(
            routing_mismatch(&paths, &store, &doc.to_string()),
            "An obsolete internal route sent to a remote provider still needs repair"
        );
        doc["model"] = value(route);
        assert!(routing_mismatch(&paths, &store, &doc.to_string()));
    }

    #[test]
    fn active_profile_overrides_root_for_route_catalog_and_provider() {
        let (_directory, paths, store) = fixture();
        let route = super::super::entries(&store.profiles)[0].route_id.clone();
        let normal = super::super::render(
            "profile=\"work\"\n[profiles.work]\n",
            &paths,
            &store,
            &route,
        )
        .unwrap();
        let mut doc = normal.parse::<DocumentMut>().unwrap();
        doc["profiles"]["work"]["model_provider"] = value("thirdparty");
        assert!(routing_mismatch(&paths, &store, &doc.to_string()));
        assert_eq!(configured_route(&store, &doc.to_string()), Some(route));
        doc["profiles"]["work"]["model"] = value("user-model");
        doc["profiles"]["work"]["model_catalog_json"] = value("user-models.json");
        assert!(
            !routing_mismatch(&paths, &store, &doc.to_string()),
            "Unselected root must not override effective custom settings"
        );
        assert_eq!(configured_route(&store, &doc.to_string()), None);
    }

    #[test]
    fn gateway_helper_with_a_remote_endpoint_is_still_a_mismatch() {
        let (_directory, paths, store) = fixture();
        let route = super::super::entries(&store.profiles)[0].route_id.clone();
        let mut doc = super::super::render("", &paths, &store, &route)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        doc["model_providers"]["ahaX"]["base_url"] = value("https://example.test/v1");
        assert!(routing_mismatch(&paths, &store, &doc.to_string()));
        doc["model_providers"]["ahaX"]["base_url"] = value("http://127.0.0.1:18761/v1");
        doc["model_providers"]["ahaX"]["auth"]["command"] = value("other-helper");
        assert!(routing_mismatch(&paths, &store, &doc.to_string()));
    }

    #[test]
    fn upgrade_preserves_native_choices_and_pending_settings_without_applying_new_models() {
        use sha2::{Digest, Sha256};
        let (_directory, paths, mut store) = fixture();
        let route = super::super::entries(&store.profiles)[0].route_id.clone();
        let legacy = route.replacen("ahax-", "vela-", 1);
        store.settings.provider_name = "Vela".into();
        let mut previous = super::super::render(
            "profile=\"work\"\n[profiles.work]\n",
            &paths,
            &store,
            &legacy,
        )
        .unwrap()
        .parse::<DocumentMut>()
        .unwrap();
        let mut catalog = super::super::model_json(&store);
        catalog["models"][0]["slug"] = json!(legacy);
        let bytes = serde_json::to_vec(&catalog).unwrap();
        let old_catalog = paths
            .data
            .join("catalogs")
            .join(format!("models-{:x}.json", Sha256::digest(&bytes)));
        crate::core::atomic_write(&old_catalog, &bytes).unwrap();
        previous["model_catalog_json"] = value(old_catalog.to_string_lossy().as_ref());
        previous["profiles"]["work"]["model_catalog_json"] =
            value(old_catalog.to_string_lossy().as_ref());
        previous["model_providers"]["Vela"]["auth"]["command"] = value(
            paths
                .helper
                .with_file_name("vela.exe")
                .to_string_lossy()
                .as_ref(),
        );
        previous["model_reasoning_effort"] = value("high");
        previous["profiles"]["work"]["model_reasoning_effort"] = value("xhigh");
        previous["model_reasoning_summary"] = value("detailed");
        previous["model_providers"]["Vela"]["request_max_retries"] = value(1);
        previous["profiles"]["untouched"] = toml_edit::Item::Table(toml_edit::Table::new());
        previous["profiles"]["untouched"]["model"] = value("other");
        store.settings.provider_name = "ahaX".into();
        store.settings.gateway_port = 23456;
        let mut pending = store.profiles[0].models[0].clone();
        pending.id = "not-applied-yet".into();
        store.profiles[0].models.push(pending);
        let upgraded = upgraded_configuration(&paths, &store, &previous)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(upgraded["model"].as_str(), Some(route.as_str()));
        assert_eq!(upgraded["model_provider"].as_str(), Some("ahaX"));
        assert_eq!(
            upgraded["profiles"]["work"]["model_reasoning_effort"].as_str(),
            Some("xhigh")
        );
        assert_eq!(upgraded["model_reasoning_effort"].as_str(), Some("high"));
        assert_eq!(
            upgraded["model_reasoning_summary"].as_str(),
            Some("detailed")
        );
        assert_eq!(
            upgraded["model_providers"]["ahaX"]["base_url"].as_str(),
            Some("http://127.0.0.1:18761/v1")
        );
        assert_eq!(
            upgraded["model_providers"]["Vela"]["auth"]["command"].as_str(),
            Some(paths.helper.to_string_lossy().as_ref())
        );
        assert_eq!(
            upgraded["model_providers"]["ahaX"]["request_max_retries"].as_integer(),
            Some(1)
        );
        assert_eq!(
            upgraded["profiles"]["untouched"]["model"].as_str(),
            Some("other")
        );
        let migrated: serde_json::Value = serde_json::from_slice(
            &std::fs::read(upgraded["model_catalog_json"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(migrated["models"].as_array().unwrap().len(), 1);
        assert_eq!(migrated["models"][0]["slug"], route);
        assert!(
            std::fs::read_to_string(previous["model_catalog_json"].as_str().unwrap())
                .unwrap()
                .contains("vela-")
        );
    }

    #[test]
    fn direct_provider_and_external_catalog_remain_byte_identical_during_startup_upgrade() {
        let (_directory, paths, store) = fixture();
        crate::core::save_store(&paths, &store).unwrap();
        let source = "# keep exact\nmodel=\"upstream\"\nmodel_provider=\"thirdparty\"\nmodel_catalog_json=\"custom.json\"\n[model_providers.thirdparty]\nbase_url=\"https://example.test/v1\"\nwire_api=\"responses\"\n";
        std::fs::write(&paths.config, source).unwrap();
        assert!(!upgrade_legacy_connection(&paths).unwrap());
        assert_eq!(std::fs::read_to_string(&paths.config).unwrap(), source);
        assert!(crate::core::load_store(&paths).unwrap().backups.is_empty());
    }

    fn old_managed_configuration(paths: &AppPaths, store: &mut Store) -> DocumentMut {
        store.settings.provider_name = "Vela".into();
        let legacy = super::super::entries(&store.profiles)[0]
            .route_id
            .replacen("ahax-", "vela-", 1);
        let mut doc = super::super::render("", paths, store, &legacy)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        doc["model_providers"]["Vela"]["auth"]["command"] = value(
            paths
                .helper
                .with_file_name("vela.exe")
                .to_string_lossy()
                .as_ref(),
        );
        store.settings.provider_name = "ahaX".into();
        doc
    }

    #[cfg(windows)]
    #[test]
    fn inactive_managed_provider_is_rebound_once_without_changing_direct_configuration() {
        let (_directory, paths, mut store) = fixture();
        let mut previous = old_managed_configuration(&paths, &mut store);
        previous["model"] = value("direct-model");
        previous["model_provider"] = value("thirdparty");
        previous["model_catalog_json"] = value("custom.json");
        previous["model_providers"]["thirdparty"] = Item::Table(toml_edit::Table::new());
        previous["model_providers"]["thirdparty"]["base_url"] = value("https://example.test/v1");
        previous["profiles"] = Item::Table(toml_edit::Table::new());
        previous["profiles"]["history"] = Item::Table(toml_edit::Table::new());
        previous["profiles"]["history"]["model_provider"] = value("Vela");
        previous["profiles"]["history"]["model"] = value("legacy-session-model");
        previous["model_providers"]["Vela"]["name"] = value("Personal local gateway");
        previous["model_providers"]["Vela"]["base_url"] = value("http://127.0.0.1:19551/v1");
        crate::core::save_store(&paths, &store).unwrap();
        std::fs::write(&paths.helper, b"isolated fixture").unwrap();
        std::fs::write(&paths.config, previous.to_string()).unwrap();
        let mut expected = previous.clone();
        expected["model_providers"]["Vela"]["auth"]["command"] =
            value(paths.helper.to_string_lossy().as_ref());
        assert!(upgrade_legacy_connection(&paths).unwrap());
        assert_eq!(
            std::fs::read_to_string(&paths.config).unwrap(),
            expected.to_string()
        );
        assert_eq!(crate::core::load_store(&paths).unwrap().backups.len(), 1);
        assert!(!upgrade_legacy_connection(&paths).unwrap());
        assert_eq!(crate::core::load_store(&paths).unwrap().backups.len(), 1);
        assert!(!paths.data.join("catalogs").exists());
    }

    #[cfg(windows)]
    #[test]
    fn disabled_route_keeps_its_config_but_still_receives_a_working_helper() {
        let (_directory, paths, mut store) = fixture();
        let previous = old_managed_configuration(&paths, &mut store);
        store.profiles[0].models[0].enabled = false;
        crate::core::save_store(&paths, &store).unwrap();
        std::fs::write(&paths.helper, b"isolated fixture").unwrap();
        std::fs::write(&paths.config, previous.to_string()).unwrap();
        let mut expected = previous;
        expected["model_providers"]["Vela"]["auth"]["command"] =
            value(paths.helper.to_string_lossy().as_ref());
        assert!(upgrade_legacy_connection(&paths).unwrap());
        assert_eq!(
            std::fs::read_to_string(&paths.config).unwrap(),
            expected.to_string()
        );
        assert!(!upgrade_legacy_connection(&paths).unwrap());
        assert_eq!(crate::core::load_store(&paths).unwrap().backups.len(), 1);
        assert!(!paths.data.join("catalogs").exists());
    }

    #[cfg(windows)]
    #[test]
    fn damaged_catalog_does_not_prevent_a_single_safe_helper_rebind() {
        let (_directory, paths, mut store) = fixture();
        let previous = old_managed_configuration(&paths, &mut store);
        crate::core::save_store(&paths, &store).unwrap();
        crate::core::atomic_write(
            Path::new(previous["model_catalog_json"].as_str().unwrap()),
            b"invalid-json",
        )
        .unwrap();
        std::fs::write(&paths.helper, b"isolated fixture").unwrap();
        std::fs::write(&paths.config, previous.to_string()).unwrap();
        let mut expected = previous;
        expected["model_providers"]["Vela"]["auth"]["command"] =
            value(paths.helper.to_string_lossy().as_ref());
        let error = upgrade_legacy_connection(&paths).unwrap_err();
        assert!(error.contains("凭据程序已更新"));
        assert_eq!(
            std::fs::read_to_string(&paths.config).unwrap(),
            expected.to_string()
        );
        assert!(upgrade_legacy_connection(&paths).is_err());
        assert_eq!(crate::core::load_store(&paths).unwrap().backups.len(), 1);
    }

    #[test]
    fn remote_or_unrelated_helpers_are_never_rebound() {
        let (_directory, paths, mut store) = fixture();
        let original = old_managed_configuration(&paths, &mut store);
        let mut remote = original.clone();
        remote["model_providers"]["Vela"]["base_url"] = value("https://example.test/v1");
        let before = remote.to_string();
        assert!(!rebind_legacy_helpers(&paths, &mut remote));
        assert_eq!(remote.to_string(), before);
        let mut unrelated = original;
        unrelated["model_providers"]["Vela"]["auth"]["command"] =
            value(paths.data.join("other/vela.exe").to_string_lossy().as_ref());
        let before = unrelated.to_string();
        assert!(!rebind_legacy_helpers(&paths, &mut unrelated));
        assert_eq!(unrelated.to_string(), before);
    }
}
