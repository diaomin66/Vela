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
        || !candidate
            .parent()
            .is_some_and(|parent| same_path(parent, &paths.data.join("catalogs")))
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

fn effective_gateway_provider(paths: &AppPaths, doc: &DocumentMut) -> bool {
    let Some(provider) = effective(doc, "model_provider")
        .and_then(Item::as_str)
        .and_then(|name| doc.get("model_providers")?.get(name))
    else {
        return false;
    };
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
            .is_some_and(|command| same_path(Path::new(command), &paths.helper))
        && auth
            .and_then(|auth| auth.get("args"))
            .and_then(Item::as_array)
            .is_some_and(|args| {
                args.len() == 1
                    && args.get(0).and_then(toml_edit::Value::as_str)
                        == Some("--gateway-credential")
            })
}

pub(crate) fn configured_route(store: &Store, contents: &str) -> Option<String> {
    let doc = contents.parse::<DocumentMut>().ok()?;
    let model = effective(&doc, "model").and_then(Item::as_str)?;
    super::entries(&store.profiles)
        .into_iter()
        .find(|entry| entry.route_id == model && entry.enabled)
        .map(|entry| entry.route_id)
}

pub(crate) fn routing_mismatch(paths: &AppPaths, store: &Store, contents: &str) -> bool {
    let Ok(doc) = contents.parse::<DocumentMut>() else {
        return false;
    };
    let known_route = effective(&doc, "model")
        .and_then(Item::as_str)
        .is_some_and(|model| {
            super::entries(&store.profiles)
                .iter()
                .any(|entry| entry.route_id == model)
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
            helper: directory.path().join("vela.exe"),
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
            "Owned picker routes still need Vela"
        );
        doc["model_catalog_json"] = value("thirdparty-models.json");
        assert!(!routing_mismatch(&paths, &store, &doc.to_string()));
        doc["model"] = value(format!("vela-{}", "f".repeat(64)));
        assert!(
            !routing_mismatch(&paths, &store, &doc.to_string()),
            "Do not guess ownership from a prefix"
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
        doc["model_providers"]["Vela"]["base_url"] = value("https://example.test/v1");
        assert!(routing_mismatch(&paths, &store, &doc.to_string()));
        doc["model_providers"]["Vela"]["base_url"] = value("http://127.0.0.1:18761/v1");
        doc["model_providers"]["Vela"]["auth"]["command"] = value("other-helper");
        assert!(routing_mismatch(&paths, &store, &doc.to_string()));
    }
}
