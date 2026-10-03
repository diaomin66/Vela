//! The local model file follows openai/codex ModelInfo + ModelsResponse.
//! Source checked: codex-rs/protocol/src/openai_models.rs (2026-10-03).
use crate::core::{self, AppPaths, Backup, Change, ChangePreview, NativeReasoning, Profile, Store};
use crate::gateway::{GatewayCatalog, GatewayRoute};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use toml_edit::{value, Array, DocumentMut, Item, Table};
use zeroize::Zeroizing;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogEntry {
    pub route_id: String,
    pub profile_id: String,
    pub channel_name: String,
    pub model_id: String,
    pub display_name: String,
    pub enabled: bool,
    pub supported_reasoning_efforts: Vec<String>,
    pub api_reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: Option<String>,
    pub native_reasoning: Option<NativeReasoning>,
}

pub fn entries(profiles: &[Profile]) -> Vec<ModelCatalogEntry> {
    profiles
        .iter()
        .flat_map(|profile| {
            profile.models.iter().map(move |model| {
                let reasoning = crate::reasoning::capabilities(model);
                let mut hash = Sha256::new();
                hash.update(profile.id.as_bytes());
                hash.update([0]);
                hash.update(model.id.as_bytes());
                let name = if model.alias.is_empty() {
                    profile.name.clone()
                } else {
                    format!("{} · {}", profile.name, model.alias)
                };
                ModelCatalogEntry {
                    route_id: format!("vela-{:x}", hash.finalize()),
                    profile_id: profile.id.clone(),
                    channel_name: profile.name.clone(),
                    model_id: model.id.clone(),
                    display_name: format!("{}（{}）", name, model.id),
                    enabled: model.enabled,
                    supported_reasoning_efforts: reasoning.efforts,
                    api_reasoning_efforts: crate::reasoning::api_efforts(model),
                    default_reasoning_effort: reasoning.default,
                    native_reasoning: reasoning.native,
                }
            })
        })
        .collect()
}
pub fn runtime_catalog(store: &Store) -> GatewayCatalog {
    GatewayCatalog {
        entries: entries(&store.profiles)
            .into_iter()
            .filter(|e| e.enabled)
            .filter_map(|e| {
                let profile = store.profiles.iter().find(|p| p.id == e.profile_id)?;
                let base = profile
                    .resolved_base_url
                    .as_deref()
                    .unwrap_or(&profile.base_url);
                let base = crate::discovery::normalize_base_url(base).ok()?;
                Some(GatewayRoute {
                    route_id: e.route_id,
                    display_name: e.display_name,
                    profile_id: e.profile_id,
                    upstream_model: e.model_id,
                    base_url: base,
                })
            })
            .collect(),
    }
}
pub fn model_file(paths: &AppPaths, store: &Store) -> PathBuf {
    let bytes = serde_json::to_vec(&model_json(store)).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    paths
        .data
        .join("catalogs")
        .join(format!("models-{:x}.json", digest))
}
pub fn model_json(store: &Store) -> Value {
    // Released Codex 0.137 requires supports_reasoning_summaries. Current upstream
    // uses supports_reasoning_summary_parameter instead; provide both spellings.
    let models:Vec<Value>=entries(&store.profiles).into_iter().filter(|e|e.enabled).enumerate().map(|(i,e)|json!({
        "slug":e.route_id,"display_name":e.display_name,"description":format!("{} · {}",e.channel_name,e.model_id),
        "default_reasoning_level":e.default_reasoning_effort,"supported_reasoning_levels":crate::reasoning::presets(&e.supported_reasoning_efforts),"shell_type":"unified_exec","visibility":"list","supported_in_api":true,"priority":i,
        "multi_agent_version":e.native_reasoning.as_ref().map(|native|native.multi_agent_version.as_str()),
        "multi_agent_reasoning_effort":e.native_reasoning.as_ref().map(|native|native.ultra_effort.as_str()),
        "availability_nux":null,"upgrade":null,"support_verbosity":false,"default_verbosity":null,"apply_patch_tool_type":null,
        "truncation_policy":{"mode":"bytes","limit":10000},"experimental_supported_tools":[],"input_modalities":["text"],
        // In 0.137 the legacy summaries flag gates the entire reasoning object.
        // New clients have a summary-only flag. Keep summaries off by default,
        // while allowing effort through on both client generations.
        "default_reasoning_summary":"none","supports_reasoning_summary_parameter":false,"supports_reasoning_summaries":!e.supported_reasoning_efforts.is_empty(),"supports_parallel_tool_calls":false,"base_instructions":include_str!("../resources/official-codex-fallback-prompt.md")
    })).collect();
    json!({"models":models})
}
pub fn write_model_catalog(paths: &AppPaths, store: &Store) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(&model_json(store)).map_err(|_| "无法生成模型目录。")?;
    core::atomic_write(&model_file(paths, store), &data)
}
fn selected_route(store: &Store, requested: Option<&str>) -> Result<String, String> {
    let entries = entries(&store.profiles);
    let requested = requested.or(store.default_route_id.as_deref());
    if let Some(id) = requested {
        if entries.iter().any(|e| e.route_id == id && e.enabled) {
            return Ok(id.into());
        }
        if requested == store.default_route_id.as_deref() {
            return entries
                .iter()
                .find(|e| e.enabled)
                .map(|e| e.route_id.clone())
                .ok_or("请至少启用一个模型。".into());
        }
        return Err("默认模型不存在或未启用。".into());
    }
    entries
        .iter()
        .find(|e| e.enabled)
        .map(|e| e.route_id.clone())
        .ok_or("请至少启用一个模型。".into())
}
pub fn render(
    current: &str,
    paths: &AppPaths,
    store: &Store,
    route: &str,
) -> Result<String, String> {
    let mut doc = current
        .parse::<DocumentMut>()
        .map_err(|_| "配置语法损坏，请先诊断或恢复。")?;
    let provider = &store.settings.provider_name;
    for key in ["model_providers", "profiles"] {
        if doc.get(key).is_some_and(|i| !i.is_table()) {
            return Err("配置表格式异常，无法安全应用网关。".into());
        }
    }
    doc["model_provider"] = value(provider);
    doc["model"] = value(route);
    doc["model_catalog_json"] = value(model_file(paths, store).to_string_lossy().as_ref());
    // A previous provider's fixed effort can override a model's native default,
    // or send unsupported reasoning parameters to a newly selected model.
    // The catalog supplies each model's default; the native picker owns changes.
    for key in [
        "model_reasoning_effort",
        "model_reasoning_summary",
        "model_supports_reasoning_summaries",
    ] {
        doc.remove(key);
    }
    if let Some(name) = doc.get("profile").and_then(Item::as_str).map(str::to_owned) {
        if !doc.contains_key("profiles") {
            doc["profiles"] = Item::Table(Table::new());
        }
        if doc["profiles"].get(&name).is_none() {
            doc["profiles"][&name] = Item::Table(Table::new());
        }
        if !doc["profiles"][&name].is_table() {
            return Err("当前 profile 不是配置表。".into());
        }
        doc["profiles"][&name]["model_provider"] = value(provider);
        doc["profiles"][&name]["model"] = value(route);
        for key in [
            "model_reasoning_effort",
            "model_reasoning_summary",
            "model_supports_reasoning_summaries",
        ] {
            doc["profiles"][&name].as_table_mut().unwrap().remove(key);
        }
    }
    if !doc.contains_key("model_providers") {
        doc["model_providers"] = Item::Table(Table::new());
    }
    if let Some(existing) = doc["model_providers"].get(provider) {
        let managed = existing
            .get("auth")
            .and_then(|a| a.get("args"))
            .and_then(Item::as_array)
            .is_some_and(|a| {
                a.len() == 1
                    && a.get(0).and_then(toml_edit::Value::as_str) == Some("--gateway-credential")
            });
        if !managed {
            return Err(
                "配置中已存在同名的其他服务商，请在设置中选择不同的 Vela 服务商名称。".into(),
            );
        }
    }
    let mut table = Table::new();
    table.insert("name", value(provider));
    table.insert(
        "base_url",
        value(format!(
            "http://127.0.0.1:{}/v1",
            store.settings.gateway_port
        )),
    );
    table.insert("wire_api", value("responses"));
    table.insert("supports_websockets", value(false));
    // A second Responses POST can be billable even if the first stream was lost.
    table.insert("request_max_retries", value(0));
    table.insert("stream_max_retries", value(0));
    let mut auth = Table::new();
    auth.insert("command", value(paths.helper.to_string_lossy().as_ref()));
    let mut args = Array::new();
    args.push("--gateway-credential");
    auth.insert("args", value(args));
    auth.insert("timeout_ms", value(35000));
    table.insert("auth", Item::Table(auth));
    doc["model_providers"][provider] = Item::Table(table);
    Ok(doc.to_string())
}
fn fingerprint(
    current: &Option<Zeroizing<Vec<u8>>>,
    proposed: &str,
    store: &Store,
) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(if current.is_some() {
        b"exists".as_slice()
    } else {
        b"missing".as_slice()
    });
    if let Some(bytes) = current {
        hasher.update(bytes.as_slice());
    }
    hasher.update(proposed.as_bytes());
    hasher.update(serde_json::to_vec(&store.settings).map_err(|_| "无法读取设置。")?);
    hasher.update(store.settings_revision.as_bytes());
    for profile in &store.profiles {
        hasher.update(profile.id.as_bytes());
        hasher.update(profile.revision.as_bytes());
    }
    hasher.update(serde_json::to_vec(&model_json(store)).map_err(|_| "无法读取模型目录。")?);
    Ok(format!("{:x}", hasher.finalize()))
}
pub fn preview(paths: &AppPaths, default: Option<&str>) -> Result<ChangePreview, String> {
    let store = core::load_store(paths)?;
    let route = selected_route(&store, default)?;
    let current = core::read_config(paths)?;
    let proposed = render(core::config_text(&current)?, paths, &store, &route)?;
    Ok(ChangePreview{id:uuid::Uuid::new_v4().to_string(),title:"同步所有模型到 Codex".into(),summary:"应用统一服务商与模型目录。窗口关闭后 Vela 在托盘中继续转发请求；请重新打开 Codex 读取模型列表。".into(),changes:vec![Change{label:"服务商".into(),before:"当前配置".into(),after:store.settings.provider_name.clone()},Change{label:"可选模型".into(),before:"当前模型目录".into(),after:format!("{} 个已启用模型",entries(&store.profiles).iter().filter(|e|e.enabled).count())},Change{label:"默认模型".into(),before:"当前默认模型".into(),after:entries(&store.profiles).iter().find(|e|e.route_id==route).map(|e|e.display_name.clone()).unwrap_or_default()}],expected_hash:fingerprint(&current,&proposed,&store)?,profile_id:None,backup_id:None})
}
pub fn apply(paths: &AppPaths, default: Option<&str>, expected: &str) -> Result<Backup, String> {
    let _lock = paths.lock()?;
    let mut store = core::load_store(paths)?;
    let route = selected_route(&store, default)?;
    let current = core::read_config(paths)?;
    let proposed = render(core::config_text(&current)?, paths, &store, &route)?;
    if fingerprint(&current, &proposed, &store)? != expected {
        return Err("配置、渠道或设置已变化，请重新预览。".into());
    }
    if !paths.helper.is_file() {
        return Err("凭据程序不存在，请重新安装。".into());
    }
    for profile in &store.profiles {
        if profile.models.iter().any(|m| m.enabled) {
            let _key = Zeroizing::new(crate::security::get_secret(&profile.id)?);
        }
    }
    write_model_catalog(paths, &store)?;
    let backup = core::commit_config(
        paths,
        current,
        Some(proposed.as_bytes()),
        "同步模型目录",
        "应用统一服务商与模型目录",
    )?;
    store = core::load_store(paths)?;
    store.default_route_id = Some(route);
    core::save_store(paths, &store)?;
    Ok(backup)
}
pub fn applied(paths: &AppPaths, store: &Store, contents: &str) -> bool {
    // Native pickers own the current model/effort. Vela's saved default is used
    // only when explicitly applying a catalog, not to invalidate native choices.
    let Some(mut actual) = contents.parse::<DocumentMut>().ok() else {
        return false;
    };
    let Some(route) = normalize_native_choices(&mut actual, store) else {
        return false;
    };
    let Ok(expected) = render(contents, paths, store, &route) else {
        return false;
    };
    let equivalent =
        Some(actual.to_string()) == expected.parse::<DocumentMut>().ok().map(|d| d.to_string());
    let current = std::fs::read(model_file(paths, store))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    equivalent && current == Some(model_json(store))
}

fn config_string(item: Option<&Item>) -> Result<Option<&str>, ()> {
    item.map(|item| item.as_str().ok_or(())).transpose()
}

fn supports_effort(entry: &ModelCatalogEntry, effort: &str) -> bool {
    entry
        .supported_reasoning_efforts
        .iter()
        .any(|value| value == effort)
        || (entry.supported_reasoning_efforts.is_empty() && effort == "none")
}

/// Normalize only proven-valid native choices for comparison; never write them
/// to disk. A profile inherits each missing value independently from the root.
fn normalize_native_choices(doc: &mut DocumentMut, store: &Store) -> Option<String> {
    let entries = entries(&store.profiles);
    let profile = config_string(doc.get("profile")).ok()?.map(str::to_owned);
    let selected = match &profile {
        Some(name) => Some(doc.get("profiles")?.get(name)?.as_table()?),
        None => None,
    };
    let root_model = config_string(doc.get("model")).ok()?;
    let selected_model = config_string(selected.and_then(|table| table.get("model"))).ok()?;
    let root_provider = config_string(doc.get("model_provider")).ok()?;
    let selected_provider =
        config_string(selected.and_then(|table| table.get("model_provider"))).ok()?;
    let root_effort = config_string(doc.get("model_reasoning_effort")).ok()?;
    let selected_effort =
        config_string(selected.and_then(|table| table.get("model_reasoning_effort"))).ok()?;

    // Validate both configured scopes, even if a profile shadows a broken root.
    // Efforts are checked against their own model before checking inheritance.
    for id in [root_model, selected_model].into_iter().flatten() {
        if !entries
            .iter()
            .any(|entry| entry.route_id == id && entry.enabled)
        {
            return None;
        }
    }
    for provider in [root_provider, selected_provider].into_iter().flatten() {
        if provider != store.settings.provider_name {
            return None;
        }
    }
    if selected_provider.or(root_provider)? != store.settings.provider_name {
        return None;
    }
    let route = selected_model.or(root_model)?;
    let effective = entries
        .iter()
        .find(|entry| entry.route_id == route && entry.enabled)?;
    if let Some(effort) = root_effort {
        if !crate::reasoning::EFFORTS.contains(&effort) {
            return None;
        }
        if let Some(root) =
            root_model.and_then(|id| entries.iter().find(|entry| entry.route_id == id))
        {
            if !supports_effort(root, effort) {
                return None;
            }
        }
    }
    if selected_effort
        .or(root_effort)
        .is_some_and(|effort| !supports_effort(effective, effort))
    {
        return None;
    }
    let route = route.to_owned();
    doc["model"] = value(&route);
    doc["model_provider"] = value(&store.settings.provider_name);
    doc.remove("model_reasoning_effort");
    if let Some(name) = profile {
        let table = doc.get_mut("profiles")?.get_mut(&name)?.as_table_mut()?;
        table["model"] = value(&route);
        table["model_provider"] = value(&store.settings.provider_name);
        table.remove("model_reasoning_effort");
    }
    Some(route)
}
pub fn is_gateway_config(contents: &str) -> bool {
    contents.parse::<DocumentMut>().ok().is_some_and(|doc| {
        let selected = doc
            .get("profile")
            .and_then(Item::as_str)
            .and_then(|name| doc.get("profiles").and_then(|p| p.get(name)));
        selected
            .and_then(|p| p.get("model_provider"))
            .or_else(|| doc.get("model_provider"))
            .and_then(Item::as_str)
            .and_then(|name| {
                doc.get("model_providers")
                    .and_then(|providers| providers.get(name))
            })
            .and_then(|p| p.get("auth"))
            .and_then(|auth| auth.get("args"))
            .and_then(Item::as_array)
            .is_some_and(|args| {
                args.len() == 1
                    && args.get(0).and_then(toml_edit::Value::as_str)
                        == Some("--gateway-credential")
            })
    })
}
pub fn configured_port(contents: &str) -> Option<u16> {
    let doc = contents.parse::<DocumentMut>().ok()?;
    let selected = doc
        .get("profile")
        .and_then(Item::as_str)
        .and_then(|name| doc.get("profiles").and_then(|p| p.get(name)));
    let provider = selected
        .and_then(|p| p.get("model_provider"))
        .or_else(|| doc.get("model_provider"))
        .and_then(Item::as_str)?;
    let provider = doc.get("model_providers")?.get(provider)?;
    if !provider
        .get("auth")?
        .get("args")?
        .as_array()?
        .iter()
        .any(|arg| arg.as_str() == Some("--gateway-credential"))
    {
        return None;
    }
    let url = url::Url::parse(provider.get("base_url")?.as_str()?).ok()?;
    if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") {
        return None;
    }
    url.port()
}

/// Validate a historical catalog before allowing a configuration restore. Files must
/// remain inside our catalog directory and every route must still identify a saved channel.
pub fn restored_store(
    paths: &AppPaths,
    contents: &str,
    current: &Store,
) -> Result<Option<Store>, String> {
    if !is_gateway_config(contents) {
        return Ok(None);
    }
    let doc = contents
        .parse::<DocumentMut>()
        .map_err(|_| "备份配置语法无效。")?;
    let selected = doc
        .get("profile")
        .and_then(Item::as_str)
        .and_then(|name| doc.get("profiles").and_then(|p| p.get(name)));
    let provider = selected
        .and_then(|p| p.get("model_provider"))
        .or_else(|| doc.get("model_provider"))
        .and_then(Item::as_str)
        .ok_or("备份未指定网关服务商。")?;
    let route = selected
        .and_then(|p| p.get("model"))
        .or_else(|| doc.get("model"))
        .and_then(Item::as_str)
        .ok_or("备份未指定默认模型。")?;
    let table = doc
        .get("model_providers")
        .and_then(|p| p.get(provider))
        .ok_or("备份的网关服务商不存在。")?;
    let command = table
        .get("auth")
        .and_then(|a| a.get("command"))
        .and_then(Item::as_str);
    if command != Some(paths.helper.to_string_lossy().as_ref()) || !paths.helper.is_file() {
        return Err("备份引用的网关程序位置已改变，请重新应用统一模型目录。".into());
    }
    let address = table
        .get("base_url")
        .and_then(Item::as_str)
        .and_then(|v| url::Url::parse(v).ok())
        .ok_or("备份网关地址无效。")?;
    if address.scheme() != "http"
        || address.host_str() != Some("127.0.0.1")
        || address.path() != "/v1"
        || address.query().is_some()
        || address.fragment().is_some()
        || !address.username().is_empty()
        || address.password().is_some()
    {
        return Err("备份网关地址不是受控的本机地址。".into());
    }
    let port = address
        .port()
        .filter(|p| *p >= 1024)
        .ok_or("备份网关端口无效。")?;
    let target = doc
        .get("model_catalog_json")
        .and_then(Item::as_str)
        .map(PathBuf::from)
        .ok_or("备份模型目录缺失。")?;
    let root = paths
        .data
        .join("catalogs")
        .canonicalize()
        .map_err(|_| "模型目录已丢失，请重新同步。")?;
    let resolved = target
        .canonicalize()
        .map_err(|_| "历史模型目录已丢失，请重新同步。")?;
    if !resolved.starts_with(root) {
        return Err("备份引用了受控目录之外的模型文件。".into());
    }
    let bytes = std::fs::read(resolved).map_err(|_| "无法读取历史模型目录。")?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("历史模型目录过大。".into());
    }
    let json: Value = serde_json::from_slice(&bytes).map_err(|_| "历史模型目录无效。")?;
    let models = json
        .get("models")
        .and_then(Value::as_array)
        .ok_or("历史模型目录结构无效。")?;
    let known = entries(&current.profiles);
    let mut ids = std::collections::HashSet::new();
    let mut historical_reasoning = std::collections::HashMap::new();
    for model in models {
        let id = model
            .get("slug")
            .and_then(Value::as_str)
            .ok_or("历史模型缺少 ID。")?;
        if !known.iter().any(|entry| entry.route_id == id) {
            return Err("历史模型目录引用了已删除的渠道或模型，请选择其他备份。".into());
        }
        historical_reasoning.insert(id.to_owned(), historical_capabilities(model)?);
        ids.insert(id.to_owned());
    }
    if !ids.contains(route) {
        return Err("历史默认模型已不在目录中。".into());
    }
    let mut restored: Store =
        serde_json::from_value(serde_json::to_value(current).map_err(|_| "无法读取渠道记录。")?)
            .map_err(|_| "无法读取渠道记录。")?;
    restored.settings.provider_name = provider.into();
    restored.settings.gateway_port = port;
    restored.settings_revision = uuid::Uuid::new_v4().to_string();
    restored.default_route_id = Some(route.into());
    for profile in &mut restored.profiles {
        for model in &mut profile.models {
            let entry = known
                .iter()
                .find(|e| e.profile_id == profile.id && e.model_id == model.id);
            let enabled = entry.is_some_and(|e| ids.contains(&e.route_id));
            model.enabled = enabled;
            if let Some(historical) =
                entry.and_then(|entry| historical_reasoning.get(&entry.route_id))
            {
                model.reasoning_efforts = Some(historical.efforts.clone());
                model.default_reasoning_effort = historical.default.clone();
                model.native_reasoning = historical.native.clone();
                crate::reasoning::normalize_model(model)?;
            }
        }
        if profile.models.iter().any(|m| m.enabled) {
            let _key = Zeroizing::new(crate::security::get_secret(&profile.id)?);
        }
    }
    Ok(Some(restored))
}

#[derive(Debug, PartialEq, Eq)]
struct HistoricalReasoning {
    efforts: Vec<String>,
    default: Option<String>,
    native: Option<NativeReasoning>,
}

fn historical_capabilities(model: &Value) -> Result<HistoricalReasoning, String> {
    let raw = model
        .get("supported_reasoning_levels")
        .and_then(Value::as_array)
        .ok_or("历史模型的推理档位结构无效。")?;
    let efforts: Vec<String> = raw
        .iter()
        .map(|preset| {
            preset
                .get("effort")
                .and_then(Value::as_str)
                .filter(|value| crate::reasoning::EFFORTS.contains(value))
                .map(str::to_owned)
                .ok_or_else(|| "历史模型含不支持的推理档位。".to_owned())
        })
        .collect::<Result<_, _>>()?;
    let default = match model.get("default_reasoning_level") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if efforts.contains(value) => Some(value.clone()),
        _ => return Err("历史模型的默认推理档位无效。".into()),
    };
    let native = if efforts.iter().any(|effort| effort == "ultra") {
        let version = model
            .get("multi_agent_version")
            .and_then(Value::as_str)
            .filter(|value| *value == "v2")
            .ok_or("历史 Ultra 模型缺少有效的多代理版本。")?;
        let wire = match model.get("multi_agent_reasoning_effort") {
            Some(Value::String(value))
                if efforts.contains(value)
                    && ["low", "medium", "high", "xhigh", "max"].contains(&value.as_str()) =>
            {
                value.clone()
            }
            None | Some(Value::Null) => ["max", "xhigh", "high", "medium", "low"]
                .into_iter()
                .find(|value| efforts.iter().any(|effort| effort == value))
                .ok_or("历史 Ultra 模型缺少可用的底层推理档位。")?
                .to_owned(),
            _ => return Err("历史 Ultra 模型的底层推理档位无效。".into()),
        };
        Some(NativeReasoning {
            multi_agent_version: version.into(),
            ultra_effort: wire,
        })
    } else {
        None
    };
    Ok(HistoricalReasoning {
        efforts,
        default,
        native,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_catalog_is_a_real_models_response() {
        let store = Store::default();
        assert_eq!(model_json(&store), json!({"models":[]}));
    }
    #[test]
    fn no_enabled_model_cannot_be_applied() {
        assert!(selected_route(&Store::default(), None).is_err());
    }
    fn sample() -> Store {
        let profile:Profile=serde_json::from_value(json!({"id":"e7ca67be-ae4f-4ad1-bef2-035b892be342","name":"渠道 A","baseUrl":"https://example.test/v1","model":"same-model","keyStored":true,"createdAt":"now","updatedAt":"now","revision":"a","models":[{"id":"same-model","alias":"编程","enabled":true}]})).unwrap();
        Store {
            profiles: vec![profile],
            ..Default::default()
        }
    }
    #[test]
    fn routes_stay_unique_and_display_names_never_expose_internal_ids() {
        let mut store = sample();
        let mut second = store.profiles[0].clone();
        second.id = uuid::Uuid::new_v4().to_string();
        second.name = "渠道 B".into();
        store.profiles.push(second);
        let entries = entries(&store.profiles);
        assert_ne!(entries[0].route_id, entries[1].route_id);
        assert_eq!(entries[0].display_name, "渠道 A · 编程（same-model）");
        assert!(!entries[0].display_name.contains(&entries[0].route_id));
        assert_eq!(
            model_json(&store)["models"][0]["shell_type"],
            "unified_exec"
        );
    }
    #[test]
    fn native_catalog_preserves_coding_instructions_and_released_schema_compatibility() {
        let catalog = model_json(&sample());
        let model = &catalog["models"][0];
        assert_eq!(model["supports_reasoning_summaries"], false);
        assert_eq!(model["supports_reasoning_summary_parameter"], false);
        assert!(model["apply_patch_tool_type"].is_null());
        assert_eq!(
            model["truncation_policy"],
            json!({"mode":"bytes","limit":10000})
        );
        let instructions = model["base_instructions"].as_str().unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(instructions.as_bytes())),
            "ac8ae107a0d72fe3476b430afb161ea4e67da2e446d778aefc44828160559807"
        );
    }
    #[test]
    fn gateway_config_allows_cold_start_and_does_not_retry_billable_requests() {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: directory.path().join("data"),
            config: directory.path().join("config.toml"),
            helper: directory.path().join("vela.exe"),
        };
        let store = sample();
        let config = render("", &paths, &store, &entries(&store.profiles)[0].route_id)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        let provider = &config["model_providers"]["Vela"];
        assert_eq!(provider["auth"]["timeout_ms"].as_integer(), Some(35000));
        assert_eq!(provider["request_max_retries"].as_integer(), Some(0));
        assert_eq!(provider["stream_max_retries"].as_integer(), Some(0));
        assert_eq!(provider["supports_websockets"].as_bool(), Some(false));
    }
    #[test]
    fn catalog_changes_use_immutable_paths_and_applied_tracks_actual_content() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: dir.path().join("data"),
            config: dir.path().join("config.toml"),
            helper: dir.path().join("vela.exe"),
        };
        let mut store = sample();
        let route = entries(&store.profiles)[0].route_id.clone();
        store.default_route_id = Some(route.clone());
        let config = render(
            "# personal\n[projects.workspace]\ntrust_level=\"trusted\"",
            &paths,
            &store,
            &route,
        )
        .unwrap();
        write_model_catalog(&paths, &store).unwrap();
        assert!(config.contains("# personal"));
        assert!(applied(&paths, &store, &config));
        let original = model_file(&paths, &store);
        store.profiles[0].models[0].alias = "new alias".into();
        assert_ne!(original, model_file(&paths, &store));
        assert!(!applied(&paths, &store, &config));
        write_model_catalog(&paths, &store).unwrap();
        assert!(original.is_file());
    }
    #[test]
    fn gateway_never_overwrites_an_unrelated_same_name_provider() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: dir.path().join("data"),
            config: dir.path().join("config"),
            helper: dir.path().join("helper"),
        };
        let store = sample();
        assert!(render(
            "[model_providers.Vela]\nname=\"User service\"",
            &paths,
            &store,
            &entries(&store.profiles)[0].route_id
        )
        .is_err());
    }

    #[test]
    fn reasoning_catalog_exposes_native_choices_without_requiring_summary_support() {
        let mut store = sample();
        store.profiles[0].models[0].id = "gpt-5.4".into();
        let catalog = model_json(&store);
        let model = &catalog["models"][0];
        assert_eq!(model["default_reasoning_level"], "none");
        assert_eq!(
            model["supported_reasoning_levels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|preset| preset["effort"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["none", "low", "medium", "high", "xhigh"]
        );
        assert_eq!(model["supports_reasoning_summaries"], true);
        assert_eq!(model["supports_reasoning_summary_parameter"], false);
        assert_eq!(model["default_reasoning_summary"], "none");
        let entry = &entries(&store.profiles)[0];
        assert_eq!(
            entry.supported_reasoning_efforts,
            ["none", "low", "medium", "high", "xhigh"]
        );
        assert_eq!(entry.default_reasoning_effort.as_deref(), Some("none"));
    }

    #[test]
    fn apply_removes_stale_reasoning_overrides_from_root_and_selected_profile_only() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: dir.path().join("data"),
            config: dir.path().join("config"),
            helper: dir.path().join("helper"),
        };
        let store = sample();
        let current = "# preserved\nprofile=\"work\"\nmodel_reasoning_effort=\"xhigh\"\nmodel_supports_reasoning_summaries=true\nmodel_reasoning_summary=\"detailed\"\n[profiles.work]\nmodel_reasoning_effort=\"high\"\nmodel_reasoning_summary=\"auto\"\nmodel_supports_reasoning_summaries=false\nsandbox_mode=\"read-only\"\n[profiles.other]\nmodel_reasoning_effort=\"xhigh\"\n";
        let rendered = render(
            current,
            &paths,
            &store,
            &entries(&store.profiles)[0].route_id,
        )
        .unwrap();
        let doc = rendered.parse::<DocumentMut>().unwrap();
        for key in [
            "model_reasoning_effort",
            "model_reasoning_summary",
            "model_supports_reasoning_summaries",
        ] {
            assert!(doc.get(key).is_none());
            assert!(doc["profiles"]["work"].get(key).is_none());
        }
        assert_eq!(
            doc["profiles"]["work"]["sandbox_mode"].as_str(),
            Some("read-only")
        );
        assert_eq!(
            doc["profiles"]["other"]["model_reasoning_effort"].as_str(),
            Some("xhigh")
        );
        assert!(rendered.contains("# preserved"));
    }

    #[test]
    fn valid_native_effort_selection_keeps_catalog_applied_but_invalid_choice_needs_repair() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: dir.path().join("data"),
            config: dir.path().join("config"),
            helper: dir.path().join("helper"),
        };
        let mut store = sample();
        store.profiles[0].models[0].id = "gpt-5.4".into();
        let route = entries(&store.profiles)[0].route_id.clone();
        let config = render(
            "profile=\"work\"\n[profiles.work]\n",
            &paths,
            &store,
            &route,
        )
        .unwrap();
        write_model_catalog(&paths, &store).unwrap();
        let mut selected = config.parse::<DocumentMut>().unwrap();
        selected["model_reasoning_effort"] = value("high");
        selected["profiles"]["work"]["model_reasoning_effort"] = value("xhigh");
        assert!(applied(&paths, &store, &selected.to_string()));
        selected["profiles"]["work"]["model_reasoning_effort"] = value("invalid");
        assert!(!applied(&paths, &store, &selected.to_string()));
    }

    #[test]
    fn historical_reasoning_preserves_old_disabled_catalog_and_validates_default() {
        assert_eq!(
            historical_capabilities(
                &json!({"supported_reasoning_levels":[],"default_reasoning_level":null})
            )
            .unwrap(),
            HistoricalReasoning {
                efforts: vec![],
                default: None,
                native: None
            }
        );
        assert_eq!(historical_capabilities(&json!({"supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],"default_reasoning_level":"high"})).unwrap(), HistoricalReasoning { efforts: vec!["low".into(),"high".into()], default: Some("high".into()), native: None });
        assert!(historical_capabilities(&json!({"supported_reasoning_levels":[{"effort":"high"}],"default_reasoning_level":"xhigh"})).is_err());
        assert!(historical_capabilities(&json!({"supported_reasoning_levels":[{"effort":"unsupported"}],"default_reasoning_level":null})).is_err());
    }

    #[test]
    fn native_ultra_catalog_emits_runtime_and_model_specific_wire_effort() {
        let mut store = sample();
        store.profiles[0].models[0].id = "gpt-6-astra".into();
        let catalog = model_json(&store);
        let model = &catalog["models"][0];
        assert_eq!(model["multi_agent_version"], "v2");
        assert_eq!(model["multi_agent_reasoning_effort"], "xhigh");
        assert!(model["supported_reasoning_levels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|preset| preset["effort"] == "ultra"));
        assert_eq!(model["default_reasoning_level"], "low");
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: directory.path().join("data"),
            config: directory.path().join("config"),
            helper: directory.path().join("helper"),
        };
        write_model_catalog(&paths, &store).unwrap();
        let mut config = render(
            "[agents]\nenabled=false\n",
            &paths,
            &store,
            &entries(&store.profiles)[0].route_id,
        )
        .unwrap()
        .parse::<DocumentMut>()
        .unwrap();
        config["model_reasoning_effort"] = value("ultra");
        assert!(applied(&paths, &store, &config.to_string()));
        assert_eq!(config["agents"]["enabled"].as_bool(), Some(false));
        store.profiles[0].models[0].id = "gpt-6-luna".into();
        assert!(model_json(&store)["models"][0]["multi_agent_version"].is_null());
    }

    #[test]
    fn historical_ultra_keeps_wire_target_and_old_catalogs_do_not_gain_ultra() {
        let mut store = sample();
        let model = &mut store.profiles[0].models[0];
        model.id = "gpt-6-astra".into();
        let saved = json!({
            "supported_reasoning_levels":[{"effort":"high"},{"effort":"xhigh"},{"effort":"ultra"}],
            "default_reasoning_level":"ultra",
            "multi_agent_version":"v2",
            "multi_agent_reasoning_effort":"high"
        });
        let historical = historical_capabilities(&saved).unwrap();
        model.reasoning_efforts = Some(historical.efforts);
        model.default_reasoning_effort = historical.default;
        model.native_reasoning = historical.native;
        crate::reasoning::normalize_model(model).unwrap();
        let regenerated = model_json(&store);
        assert_eq!(
            regenerated["models"][0]["multi_agent_reasoning_effort"],
            "high"
        );
        assert_eq!(regenerated["models"][0]["default_reasoning_level"], "ultra");

        let old =
            historical_capabilities(&json!({"supported_reasoning_levels":[{"effort":"high"}]}))
                .unwrap();
        let model = &mut store.profiles[0].models[0];
        model.reasoning_efforts = Some(old.efforts);
        model.default_reasoning_effort = old.default;
        model.native_reasoning = old.native;
        crate::reasoning::normalize_model(model).unwrap();
        assert_eq!(
            entries(&store.profiles)[0].supported_reasoning_efforts,
            ["high"]
        );
        assert!(model_json(&store)["models"][0]["multi_agent_version"].is_null());
    }

    #[test]
    fn historical_ultra_rejects_invalid_runtime_or_missing_underlying_effort() {
        let valid = json!({
            "supported_reasoning_levels":[{"effort":"high"},{"effort":"xhigh"},{"effort":"ultra"}],
            "default_reasoning_level":"ultra",
            "multi_agent_version":"v2"
        });
        assert_eq!(
            historical_capabilities(&valid)
                .unwrap()
                .native
                .unwrap()
                .ultra_effort,
            "xhigh"
        );
        for invalid in [json!("ultra"), json!("max"), json!(42)] {
            let mut modified = valid.clone();
            modified["multi_agent_reasoning_effort"] = invalid;
            assert!(historical_capabilities(&modified).is_err());
        }
        for invalid in [Value::Null, json!("v1"), json!(42)] {
            let mut modified = valid.clone();
            modified["multi_agent_version"] = invalid;
            assert!(historical_capabilities(&modified).is_err());
        }
        let mut modified = valid;
        modified["supported_reasoning_levels"] = json!([{"effort":"ultra"}]);
        assert!(historical_capabilities(&modified).is_err());
    }

    fn disjoint_reasoning_models() -> Store {
        let mut store = sample();
        let model = &mut store.profiles[0].models[0];
        model.id = "deep-model".into();
        model.reasoning_efforts = Some(vec!["high".into(), "xhigh".into()]);
        model.default_reasoning_effort = Some("high".into());
        let mut second = model.clone();
        second.id = "fast-model".into();
        second.reasoning_efforts = Some(vec!["low".into()]);
        second.default_reasoning_effort = Some("low".into());
        store.profiles[0].models.push(second);
        store.default_route_id = Some(entries(&store.profiles)[0].route_id.clone());
        store
    }

    #[test]
    fn native_model_switch_uses_its_own_effort_without_changing_vela_default() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: dir.path().join("data"),
            config: dir.path().join("config"),
            helper: dir.path().join("helper"),
        };
        let store = disjoint_reasoning_models();
        let routes = entries(&store.profiles);
        let original = render("", &paths, &store, &routes[0].route_id).unwrap();
        write_model_catalog(&paths, &store).unwrap();
        let mut native = original.parse::<DocumentMut>().unwrap();
        native["model"] = value(&routes[1].route_id);
        native["model_reasoning_effort"] = value("low");
        assert!(applied(&paths, &store, &native.to_string()));
        assert_eq!(
            store.default_route_id.as_deref(),
            Some(routes[0].route_id.as_str())
        );
        let reapplied = render(
            &native.to_string(),
            &paths,
            &store,
            &selected_route(&store, None).unwrap(),
        )
        .unwrap()
        .parse::<DocumentMut>()
        .unwrap();
        assert_eq!(
            reapplied["model"].as_str(),
            Some(routes[0].route_id.as_str())
        );
        assert!(reapplied.get("model_reasoning_effort").is_none());
    }

    #[test]
    fn native_profile_selection_respects_per_field_inheritance_and_rejects_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: dir.path().join("data"),
            config: dir.path().join("config"),
            helper: dir.path().join("helper"),
        };
        let store = disjoint_reasoning_models();
        let routes = entries(&store.profiles);
        let original = render(
            "profile=\"work\"\n[profiles.work]\n",
            &paths,
            &store,
            &routes[0].route_id,
        )
        .unwrap();
        write_model_catalog(&paths, &store).unwrap();
        let mut native = original.parse::<DocumentMut>().unwrap();
        native["model_reasoning_effort"] = value("xhigh");
        native["profiles"]["work"]["model"] = value(&routes[1].route_id);
        native["profiles"]["work"]["model_reasoning_effort"] = value("low");
        assert!(applied(&paths, &store, &native.to_string()));
        native["profiles"]["work"]
            .as_table_mut()
            .unwrap()
            .remove("model_provider");
        assert!(
            applied(&paths, &store, &native.to_string()),
            "Provider may inherit from the managed root"
        );
        native["profiles"]["work"]
            .as_table_mut()
            .unwrap()
            .remove("model_reasoning_effort");
        assert!(
            !applied(&paths, &store, &native.to_string()),
            "Inherited xhigh is invalid for fast-model"
        );
        native["profiles"]["work"]["model_reasoning_effort"] = value("low");
        native["model_reasoning_effort"] = value("low");
        assert!(
            !applied(&paths, &store, &native.to_string()),
            "A valid profile must not hide a broken root choice"
        );
    }

    #[test]
    fn native_selection_never_masks_disabled_routes_or_managed_config_damage() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: dir.path().join("data"),
            config: dir.path().join("config"),
            helper: dir.path().join("helper"),
        };
        let mut store = disjoint_reasoning_models();
        store.profiles[0].models[1].enabled = false;
        let routes = entries(&store.profiles);
        let original = render("", &paths, &store, &routes[0].route_id).unwrap();
        write_model_catalog(&paths, &store).unwrap();
        for selected in [routes[1].route_id.as_str(), "vela-unknown"] {
            let mut native = original.parse::<DocumentMut>().unwrap();
            native["model"] = value(selected);
            assert!(!applied(&paths, &store, &native.to_string()));
        }
        for field in [
            "model_provider",
            "model",
            "model_reasoning_effort",
            "profile",
        ] {
            let mut native = original.parse::<DocumentMut>().unwrap();
            native[field] = value(42);
            assert!(
                !applied(&paths, &store, &native.to_string()),
                "Invalid {field} type must remain visible"
            );
        }
        let mut native = original.parse::<DocumentMut>().unwrap();
        native["model_providers"]["Vela"]["base_url"] = value("http://127.0.0.1:19999/v1");
        assert!(!applied(&paths, &store, &native.to_string()));
        let mut native = original.parse::<DocumentMut>().unwrap();
        native["model_providers"]["Vela"]["auth"]["command"] = value("wrong-helper");
        assert!(!applied(&paths, &store, &native.to_string()));
        let mut native = original.parse::<DocumentMut>().unwrap();
        native["model_provider"] = value("Other");
        assert!(!applied(&paths, &store, &native.to_string()));
    }

    #[test]
    fn native_none_effort_is_valid_for_a_model_without_reasoning_choices() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: dir.path().join("data"),
            config: dir.path().join("config"),
            helper: dir.path().join("helper"),
        };
        let store = sample();
        let original = render("", &paths, &store, &entries(&store.profiles)[0].route_id).unwrap();
        write_model_catalog(&paths, &store).unwrap();
        let mut native = original.parse::<DocumentMut>().unwrap();
        native["model_reasoning_effort"] = value("none");
        assert!(applied(&paths, &store, &native.to_string()));
        native["model_reasoning_effort"] = value("low");
        assert!(!applied(&paths, &store, &native.to_string()));
    }
}
