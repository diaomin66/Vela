//! Render and apply a legacy direct channel while preserving unrelated TOML.
use super::{
    backups::commit_config,
    changes::{changes, raw, token},
    domain::{Backup, ChangePreview, Profile},
    filesystem::{config_text, read_config},
    paths::AppPaths,
    profiles::load_profile,
};
use std::path::Path;
use toml_edit::{value, Array, DocumentMut, Item, Table};
use uuid::Uuid;
use zeroize::Zeroizing;

pub fn provider_id(id: &str) -> String {
    format!("vela_{}", id.replace('-', ""))
}
pub fn active_profile_id(contents: &str, profiles: &[Profile]) -> Option<String> {
    let document = contents.parse::<DocumentMut>().ok()?;
    let selected = document
        .get("profile")
        .and_then(Item::as_str)
        .and_then(|name| {
            document
                .get("profiles")
                .and_then(|profiles| profiles.get(name))
        });
    let provider = selected
        .and_then(|profile| profile.get("model_provider"))
        .or_else(|| document.get("model_provider"))?
        .as_str()?;
    profiles
        .iter()
        .find(|p| provider_id(&p.id) == provider)
        .map(|p| p.id.clone())
}
pub fn profile_matches_configuration(contents: &str, profile: &Profile, helper: &Path) -> bool {
    let Ok(doc) = contents.parse::<DocumentMut>() else {
        return false;
    };
    let selected = doc
        .get("profile")
        .and_then(Item::as_str)
        .and_then(|name| doc.get("profiles").and_then(|profiles| profiles.get(name)));
    let actual_model = selected
        .and_then(|p| p.get("model"))
        .or_else(|| doc.get("model"))
        .and_then(Item::as_str);
    let provider_name = provider_id(&profile.id);
    let Some(provider) = doc
        .get("model_providers")
        .and_then(|p| p.get(&provider_name))
    else {
        return false;
    };
    let auth = provider.get("auth");
    let command_matches = auth.and_then(|a| a.get("command")).and_then(Item::as_str)
        == Some(helper.to_string_lossy().as_ref());
    let args_matches = auth
        .and_then(|a| a.get("args"))
        .and_then(Item::as_array)
        .is_some_and(|args| {
            args.len() == 2
                && args.get(0).and_then(toml_edit::Value::as_str) == Some("--credential")
                && args.get(1).and_then(toml_edit::Value::as_str) == Some(profile.id.as_str())
        });
    actual_model == Some(profile.model.as_str())
        && provider.get("base_url").and_then(Item::as_str) == Some(profile.base_url.as_str())
        && provider.get("wire_api").and_then(Item::as_str) == Some("responses")
        && [
            "env_key",
            "experimental_bearer_token",
            "api_key",
            "requires_openai_auth",
        ]
        .iter()
        .all(|key| provider.get(key).is_none())
        && ["http_headers", "env_http_headers"].iter().all(|field| {
            provider
                .get(field)
                .and_then(Item::as_table_like)
                .is_none_or(|headers| {
                    headers
                        .iter()
                        .all(|(name, _)| !name.eq_ignore_ascii_case("authorization"))
                })
        })
        && command_matches
        && args_matches
}

pub fn render_profile(current: &str, profile: &Profile, helper: &Path) -> Result<String, String> {
    if profile.model.trim().is_empty() {
        return Err("此渠道尚未指定旧版直连模型，请使用统一模型目录选择并应用模型。".into());
    }
    let mut doc = current
        .parse::<DocumentMut>()
        .map_err(|_| "配置 TOML 存在语法错误。请先诊断或恢复备份，避免覆盖现有设置。")?;
    let provider = provider_id(&profile.id);
    if doc.get("model_providers").is_some_and(|i| !i.is_table()) {
        return Err("model_providers 不是标准表，无法安全修改。请先检查配置。".into());
    }
    doc["model"] = value(&profile.model);
    doc["model_provider"] = value(&provider);
    // A selected named profile overrides root fields. Update its connection fields too,
    // while keeping its sandbox, approval policy and other user choices intact.
    if let Some(selected) = doc.get("profile") {
        let name = selected
            .as_str()
            .ok_or("profile 字段必须为字符串，暂不能安全修改。")?
            .to_string();
        if doc.get("profiles").is_some_and(|i| !i.is_table()) {
            return Err("profiles 不是标准配置表，暂不能安全修改。".into());
        }
        if !doc.contains_key("profiles") {
            doc["profiles"] = Item::Table(Table::new());
        }
        if doc["profiles"].get(&name).is_some_and(|i| !i.is_table()) {
            return Err("选中的 profile 不是标准配置表，暂不能安全修改。".into());
        }
        if doc["profiles"].get(&name).is_none() {
            doc["profiles"][&name] = Item::Table(Table::new());
        }
        doc["profiles"][&name]["model"] = value(&profile.model);
        doc["profiles"][&name]["model_provider"] = value(&provider);
    }
    if !doc.contains_key("model_providers") {
        doc["model_providers"] = Item::Table(Table::new());
    }
    if doc["model_providers"]
        .get(&provider)
        .is_some_and(|i| !i.is_table())
    {
        return Err("此服务商配置不是标准表，无法安全修改。".into());
    }
    if doc["model_providers"].get(&provider).is_none() {
        doc["model_providers"][&provider] = Item::Table(Table::new());
    }
    let table = doc["model_providers"][&provider]
        .as_table_mut()
        .ok_or("无法编辑服务商配置。")?;
    table.insert("name", value(&profile.name));
    table.insert("base_url", value(&profile.base_url));
    table.insert("wire_api", value("responses"));
    for conflicting in [
        "env_key",
        "experimental_bearer_token",
        "api_key",
        "requires_openai_auth",
        "env_key_instructions",
    ] {
        table.remove(conflicting);
    }
    for header_field in ["http_headers", "env_http_headers"] {
        if let Some(headers) = table
            .get_mut(header_field)
            .and_then(Item::as_table_like_mut)
        {
            let authorization_keys: Vec<String> = headers
                .iter()
                .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                .map(|(name, _)| name.to_owned())
                .collect();
            for name in authorization_keys {
                headers.remove(&name);
            }
        }
    }
    let mut auth = Table::new();
    auth.insert("command", value(helper.to_string_lossy().as_ref()));
    let mut args = Array::new();
    args.push("--credential");
    args.push(profile.id.as_str());
    auth.insert("args", value(args));
    table.insert("auth", Item::Table(auth));
    let result = doc.to_string();
    result
        .parse::<DocumentMut>()
        .map_err(|_| "配置生成后的校验失败，尚未写入。")?;
    Ok(result)
}

pub fn preview_profile(paths: &AppPaths, id: &str) -> Result<ChangePreview, String> {
    let profile = load_profile(paths, id)?;
    let current = read_config(paths)?;
    let contents = config_text(&current)?;
    let proposed = Zeroizing::new(render_profile(contents, &profile, &paths.helper)?);
    Ok(ChangePreview { id: Uuid::new_v4().to_string(), title: format!("使用 {}", profile.name), summary: "将更新默认模型和当前连接，并保留其他配置。应用前会自动创建加密备份；请关闭正在运行的 Codex 后应用，再重新打开。".into(), changes: changes(contents, &proposed), expected_hash: token(raw(&current), Some(proposed.as_bytes())), profile_id: Some(id.into()), backup_id: None })
}
pub fn apply_profile(paths: &AppPaths, id: &str, expected_hash: &str) -> Result<Backup, String> {
    let _lock = paths.lock()?;
    let profile = load_profile(paths, id)?;
    let secret = Zeroizing::new(crate::security::get_secret(id)?);
    if secret.is_empty() {
        return Err("此连接未保存有效 Key。".into());
    }
    if !paths.helper.is_file() {
        return Err("凭据读取程序不存在，请重新安装应用。".into());
    }
    let current = read_config(paths)?;
    let proposed = Zeroizing::new(render_profile(
        config_text(&current)?,
        &profile,
        &paths.helper,
    )?);
    if token(raw(&current), Some(proposed.as_bytes())) != expected_hash {
        return Err("配置或连接在预览后发生变化，请重新预览再应用。".into());
    }
    commit_config(
        paths,
        current,
        Some(proposed.as_bytes()),
        "切换连接",
        &format!("应用连接：{}", profile.name),
    )
}
