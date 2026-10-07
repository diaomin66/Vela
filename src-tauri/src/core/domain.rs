//! Persisted and IPC domain data. Keep serde defaults compatible with older installs.
use crate::discovery::{BalanceConfig, BalanceSnapshot};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub key_stored: bool,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_validated_at: Option<String>,
    #[serde(default)]
    pub models: Vec<ChannelModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<BalanceSnapshot>,
    #[serde(default)]
    pub balance_config: BalanceConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChannelModel {
    pub id: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_efforts: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_reasoning: Option<NativeReasoning>,
}

/// Native picker semantics retained with restored model catalogs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeReasoning {
    pub multi_agent_version: String,
    pub ultra_effort: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub provider_name: String,
    pub gateway_port: u16,
    pub auto_refresh: bool,
    pub refresh_minutes: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            provider_name: "ahaX".into(),
            gateway_port: 18761,
            auto_refresh: true,
            refresh_minutes: 15,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInput {
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    pub api_key: Option<String>,
    pub models: Option<Vec<ChannelModel>>,
    pub balance_config: Option<BalanceConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub id: String,
    pub created_at: String,
    pub reason: String,
    pub summary: String,
    pub config_existed: bool,
    pub config_path: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub backups: Vec<Backup>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub settings_revision: String,
    #[serde(default)]
    pub default_route_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub label: String,
    pub before: String,
    pub after: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePreview {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub changes: Vec<Change>,
    pub expected_hash: String,
    pub profile_id: Option<String>,
    pub backup_id: Option<String>,
}

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
pub fn validate_id(id: &str) -> Result<(), String> {
    if Uuid::parse_str(id)
        .map(|u| u.to_string() == id)
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err("连接或备份标识无效。".into())
    }
}
