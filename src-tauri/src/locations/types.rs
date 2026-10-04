use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct LocationPreferences {
    pub codex_home: Option<String>,
    pub sqlite_home: Option<String>,
    pub backups_directory: Option<String>,
    pub evaluations_directory: Option<String>,
    pub exports_directory: Option<String>,
    pub thread_protection_directory: Option<String>,
    pub thread_index_directory: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Defaults {
    pub codex_home: PathBuf,
    pub user_home: Option<PathBuf>,
    pub environment_home: Option<PathBuf>,
    pub environment_sqlite: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct ResolvedLocations {
    pub codex_home: PathBuf,
    pub sqlite_home: PathBuf,
    pub backups_directory: PathBuf,
    pub evaluations_directory: PathBuf,
    pub exports_directory: PathBuf,
    pub thread_protection_directory: PathBuf,
    pub thread_index_directory: PathBuf,
    pub(crate) defaults: Defaults,
    pub(crate) error: Option<String>,
}

impl ResolvedLocations {
    pub(crate) fn defaults(data: &Path, home: &Path, sqlite: &Path) -> Self {
        Self {
            codex_home: home.into(),
            sqlite_home: sqlite.into(),
            backups_directory: data.join("backups"),
            evaluations_directory: data.join("evaluations"),
            exports_directory: data.join("evaluations").join("exports"),
            thread_protection_directory: data.join("threads"),
            thread_index_directory: data.join("threads"),
            defaults: Defaults {
                codex_home: home.into(),
                user_home: None,
                environment_home: None,
                environment_sqlite: None,
            },
            error: None,
        }
    }
    pub(crate) fn display(&self) -> ResolvedLocationPaths {
        use super::validation::display;
        ResolvedLocationPaths {
            codex_home: display(&self.codex_home),
            sqlite_home: display(&self.sqlite_home),
            backups_directory: display(&self.backups_directory),
            evaluations_directory: display(&self.evaluations_directory),
            exports_directory: display(&self.exports_directory),
            thread_protection_directory: display(&self.thread_protection_directory),
            thread_index_directory: display(&self.thread_index_directory),
            config_path: display(&self.codex_home.join("config.toml")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLocationPaths {
    pub codex_home: String,
    pub sqlite_home: String,
    pub backups_directory: String,
    pub evaluations_directory: String,
    pub exports_directory: String,
    pub thread_protection_directory: String,
    pub thread_index_directory: String,
    pub config_path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationOverride {
    pub key: String,
    pub environment: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationStatus {
    pub preferences: LocationPreferences,
    pub pending_preferences: Option<LocationPreferences>,
    pub active: ResolvedLocationPaths,
    pub next: ResolvedLocationPaths,
    pub requires_restart: bool,
    pub anchor_directory: String,
    pub overrides: Vec<LocationOverride>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationChange {
    pub key: String,
    pub label: String,
    pub current_path: String,
    pub next_path: String,
    pub migration: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationPreview {
    pub expected_hash: String,
    pub preferences: LocationPreferences,
    pub resolved: ResolvedLocationPaths,
    pub changes: Vec<LocationChange>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub can_save: bool,
    pub requires_restart: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Persisted {
    pub version: u32,
    pub revision: String,
    pub active: LocationPreferences,
    pub pending: Option<LocationPreferences>,
    pub pending_resolved: Option<ResolvedLocationPaths>,
    pub error: Option<String>,
    #[serde(default)]
    pub pending_sources: Vec<String>,
}
impl Default for Persisted {
    fn default() -> Self {
        Self {
            version: 1,
            revision: String::new(),
            active: LocationPreferences::default(),
            pending: None,
            pending_resolved: None,
            pending_sources: Vec::new(),
            error: None,
        }
    }
}
