//! Channel edits, credential consistency, and revision-checked metadata updates.
use super::{
    configuration::active_profile_id,
    domain::{now, validate_id, ChannelModel, Profile, ProfileInput},
    filesystem::{atomic_write, config_text, read_config},
    paths::AppPaths,
    store::{load_store, save_store},
};
use crate::discovery::DiscoveryResult;
use uuid::Uuid;
use zeroize::Zeroizing;

pub fn load_profile(paths: &AppPaths, id: &str) -> Result<Profile, String> {
    validate_id(id)?;
    load_store(paths)?
        .profiles
        .into_iter()
        .find(|p| p.id == id)
        .ok_or("此连接不存在。".into())
}
pub fn load_validation_profile(
    paths: &AppPaths,
    id: &str,
) -> Result<(Profile, Zeroizing<String>), String> {
    // Read endpoint and credential under the same mutation lock, so an edited
    // endpoint can never receive a key belonging to another saved revision.
    let _lock = paths.lock()?;
    let profile = load_profile(paths, id)?;
    let key = Zeroizing::new(crate::security::get_secret(id)?);
    Ok((profile, key))
}
pub fn save_profile(paths: &AppPaths, mut input: ProfileInput) -> Result<Profile, String> {
    let _lock = paths.lock()?;
    let mut store = load_store(paths)?;
    let name = input.name.trim().to_string();
    let model = input.model.trim().to_string();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err("连接名称需为 1–80 个可见字符。".into());
    }
    if model.chars().count() > 200 || model.chars().any(char::is_control) {
        return Err("请填写有效的模型名称。".into());
    }
    let base_url = crate::discovery::normalize_base_url(&input.base_url)?;
    let previous = match input.id.as_deref() {
        Some(id) => {
            validate_id(id)?;
            Some(
                store
                    .profiles
                    .iter()
                    .find(|p| p.id == id)
                    .cloned()
                    .ok_or("此连接不存在。")?,
            )
        }
        None => None,
    };
    // Running Codex instances may still hold the old URL + credential ID.
    // Reusing that ID for another endpoint could disclose the replacement key
    // to the old endpoint before a configuration switch or restart occurs.
    if previous
        .as_ref()
        .is_some_and(|profile| profile.base_url != base_url)
    {
        if let Some(key) = input.api_key.as_mut() {
            use zeroize::Zeroize;
            key.zeroize();
        }
        return Err(
            "已有连接的 API 地址不能直接修改。更换服务商地址时请新建连接，验证后再切换。".into(),
        );
    }
    let id = previous
        .as_ref()
        .map(|p| p.id.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let balance_config = input
        .balance_config
        .take()
        .or_else(|| previous.as_ref().map(|p| p.balance_config.clone()))
        .unwrap_or_default();
    crate::discovery::validate_balance_config(&base_url, &balance_config)?;
    let explicit_models = input.models.is_some();
    let mut models = input.models.take().unwrap_or_else(|| {
        previous
            .as_ref()
            .map(|p| p.models.clone())
            .unwrap_or_default()
    });
    if !explicit_models && models.is_empty() && !model.is_empty() {
        models.push(ChannelModel {
            id: model.clone(),
            alias: String::new(),
            enabled: true,
            reasoning_efforts: None,
            default_reasoning_effort: None,
        });
    }
    let mut seen = std::collections::HashSet::new();
    for entry in &mut models {
        entry.id = entry.id.trim().into();
        entry.alias = entry.alias.trim().into();
        crate::reasoning::normalize_model(entry)?;
        if entry.id.is_empty()
            || entry.id.len() > 240
            || entry.id.chars().any(char::is_control)
            || entry.alias.chars().any(char::is_control)
            || entry.alias.chars().count() > 100
            || !seen.insert(entry.id.clone())
        {
            return Err("模型 ID 必须唯一且有效，别名不能包含控制字符。".into());
        }
    }
    let new_key = input
        .api_key
        .take()
        .map(Zeroizing::new)
        .filter(|s| !s.is_empty());
    if previous.is_none() && new_key.is_none() {
        return Err("首次保存连接时请填写 API Key。".into());
    }
    if let Some(key) = &new_key {
        if key.trim() != key.as_str() || key.chars().any(char::is_control) {
            return Err("Key 不能包含首尾空格、换行或控制字符。".into());
        }
    }
    let old_key = if new_key.is_some() && previous.is_some() {
        crate::security::get_secret(&id).ok().map(Zeroizing::new)
    } else {
        None
    };
    if let Some(key) = &new_key {
        crate::security::set_secret(&id, key)?;
    }
    let timestamp = now();
    let profile = Profile {
        id: id.clone(),
        name,
        base_url,
        model,
        key_stored: new_key.is_some() || previous.as_ref().map(|p| p.key_stored).unwrap_or(false),
        created_at: previous
            .as_ref()
            .map(|p| p.created_at.clone())
            .unwrap_or_else(|| timestamp.clone()),
        updated_at: timestamp,
        revision: Uuid::new_v4().to_string(),
        last_validated_at: None,
        models,
        resolved_base_url: previous.as_ref().and_then(|p| p.resolved_base_url.clone()),
        balance: previous.as_ref().and_then(|p| p.balance.clone()),
        balance_config,
        last_synced_at: previous.as_ref().and_then(|p| p.last_synced_at.clone()),
        sync_error: None,
    };
    if let Some(existing) = store.profiles.iter_mut().find(|p| p.id == id) {
        *existing = profile.clone();
    } else {
        store.profiles.push(profile.clone());
    }
    if let Err(error) = save_store(paths, &store) {
        if new_key.is_some() {
            if let Some(key) = &old_key {
                let _ = crate::security::set_secret(&id, key);
            } else {
                let _ = crate::security::remove_secret(&id);
            }
        }
        return Err(error);
    }
    Ok(profile)
}
pub fn delete_profile(paths: &AppPaths, id: &str) -> Result<(), String> {
    let _lock = paths.lock()?;
    validate_id(id)?;
    let mut store = load_store(paths)?;
    let current = read_config(paths)?;
    if active_profile_id(config_text(&current)?, &store.profiles).as_deref() == Some(id) {
        return Err("请先切换连接或恢复原配置，再删除当前连接。".into());
    }
    if !store.profiles.iter().any(|p| p.id == id) {
        return Err("此连接不存在。".into());
    }
    // Save metadata first: a failed metadata write must never destroy a working key.
    let original = serde_json::to_vec_pretty(&store).map_err(|_| "无法读取连接记录。")?;
    store.profiles.retain(|p| p.id != id);
    save_store(paths, &store)?;
    if let Err(error) = crate::security::remove_secret(id) {
        let _ = atomic_write(&paths.profile_file(), &original);
        return Err(error);
    }
    Ok(())
}
pub fn mark_validated(
    paths: &AppPaths,
    id: &str,
    revision: &str,
    passed: bool,
) -> Result<bool, String> {
    let _lock = paths.lock()?;
    let mut store = load_store(paths)?;
    if let Some(profile) = store.profiles.iter_mut().find(|p| p.id == id) {
        if profile.revision != revision {
            return Ok(false);
        }
        profile.last_validated_at = passed.then(now);
        save_store(paths, &store)?;
        return Ok(true);
    }
    Ok(false)
}

pub fn record_discovery(
    paths: &AppPaths,
    id: &str,
    revision: &str,
    result: DiscoveryResult,
) -> Result<Profile, String> {
    let _lock = paths.lock()?;
    let mut store = load_store(paths)?;
    let profile = store
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or("同步期间渠道已被删除。")?;
    if profile.revision != revision {
        return Err("同步期间渠道已被编辑，请重新同步。".into());
    }
    if result.models_status == "ready" {
        // Discovery never silently enables or removes a user's selected model.
        for model in result.models {
            if !profile
                .models
                .iter()
                .any(|existing| existing.id == model.id)
            {
                profile.models.push(ChannelModel {
                    id: model.id,
                    alias: String::new(),
                    enabled: false,
                    reasoning_efforts: None,
                    default_reasoning_effort: None,
                });
            }
        }
    }
    if result.resolved_base_url.is_some() {
        profile.resolved_base_url = result.resolved_base_url;
    }
    profile.balance = if profile.balance_config.mode == "disabled" {
        None
    } else if result.balance.status == "error" {
        match profile.balance.take().filter(|balance| {
            balance.remaining.is_some() && matches!(balance.status.as_str(), "available" | "stale")
        }) {
            Some(mut previous) => {
                // Keep the timestamp of the last actual measurement; the failed
                // refresh must never make stale account data appear current.
                previous.status = "stale".into();
                previous.message = result.balance.message.clone().or(result.error.clone());
                Some(previous)
            }
            None => Some(result.balance),
        }
    } else {
        Some(result.balance)
    };
    profile.last_synced_at = Some(result.checked_at);
    profile.sync_error = result.error;
    profile.updated_at = now();
    profile.revision = Uuid::new_v4().to_string();
    let saved = profile.clone();
    save_store(paths, &store)?;
    Ok(saved)
}
