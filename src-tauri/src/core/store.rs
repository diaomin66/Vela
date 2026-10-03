//! Persist channel metadata and settings, including migrations from earlier releases.
use super::{
    domain::{ChannelModel, Settings, Store},
    filesystem::atomic_write,
    paths::AppPaths,
};
use std::fs;
use uuid::Uuid;

pub fn load_store(paths: &AppPaths) -> Result<Store, String> {
    match fs::read(paths.profile_file()) {
        Ok(bytes) => {
            let mut store: Store = serde_json::from_slice(&bytes).map_err(|_| {
                "连接记录无法解析。请保留应用数据并检查 connections.json。".to_owned()
            })?;
            let raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
            for (index, profile) in store.profiles.iter_mut().enumerate() {
                let legacy = raw
                    .get("profiles")
                    .and_then(|p| p.get(index))
                    .is_some_and(|p| p.get("models").is_none());
                if legacy && profile.models.is_empty() && !profile.model.is_empty() {
                    profile.models.push(ChannelModel {
                        id: profile.model.clone(),
                        alias: String::new(),
                        enabled: true,
                        reasoning_efforts: None,
                        default_reasoning_effort: None,
                        native_reasoning: None,
                    });
                }
            }
            Ok(store)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(_) => Err("无法读取连接记录。".into()),
    }
}
pub(crate) fn save_store(paths: &AppPaths, store: &Store) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(store).map_err(|_| "无法序列化连接记录。")?;
    atomic_write(&paths.profile_file(), &bytes)
}

pub fn save_settings(paths: &AppPaths, input: Settings) -> Result<Settings, String> {
    if input.provider_name.trim() != input.provider_name
        || input.provider_name.is_empty()
        || input.provider_name.chars().count() > 48
        || input.provider_name.chars().any(|c| c.is_control())
        || matches!(
            input.provider_name.as_str(),
            "openai" | "ollama" | "lmstudio" | "amazon-bedrock"
        )
    {
        return Err("服务商名称需为 1–48 个可见字符，且不能与内置服务商重名。".into());
    }
    if input.gateway_port < 1024 || input.refresh_minutes < 1 || input.refresh_minutes > 1440 {
        return Err("网关端口需为 1024–65535；刷新间隔需为 1–1440 分钟。".into());
    }
    let _lock = paths.lock()?;
    let mut store = load_store(paths)?;
    store.settings = input.clone();
    store.settings_revision = Uuid::new_v4().to_string();
    save_store(paths, &store)?;
    Ok(input)
}
