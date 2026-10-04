use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSource {
    pub id: String,
    pub kind: String,
    pub root: String,
    pub display_root: String,
    pub available: bool,
    pub writable: bool,
    pub last_scanned_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub key: String,
    pub source_id: String,
    pub thread_id: String,
    pub path: String,
    pub relative_path: String,
    pub archived: bool,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub provider: Option<String>,
    pub source_kind: Option<String>,
    #[serde(default)]
    pub history_base: Option<ThreadHistoryBase>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub bytes: u64,
    pub line_count: u64,
    pub index_present: bool,
    #[serde(default)]
    pub state_index: Option<String>,
    #[serde(default)]
    pub selected_rollout: Option<bool>,
    #[serde(default)]
    pub last_verified_at: Option<String>,
    #[serde(default)]
    pub protected_bytes: Option<u64>,
    pub integrity: String,
    pub snapshot: SnapshotState,
    pub recoverability: String,
    pub fingerprint: String,
    pub scan_revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadHistoryBase {
    pub thread_id: String,
    pub end_ordinal_exclusive: u64,
    #[serde(default)]
    pub end_byte_offset: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetail {
    pub summary: ThreadSummary,
    pub preview: Vec<ThreadMessagePreview>,
    pub snapshot_bytes: Option<u64>,
    pub snapshot_hash: Option<String>,
    pub raw_available: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMessagePreview {
    pub role: Option<String>,
    pub text: String,
    pub timestamp: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotState {
    Protected,
    Pending,
    Missing,
    Failed,
    TooLarge,
    NotNeeded,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadProtectionStatus {
    pub state: String,
    pub last_success_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub protected_count: u64,
    pub pending_count: u64,
    pub failed_count: u64,
    pub bytes_protected: u64,
    pub current_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDashboard {
    pub sources: Vec<ThreadSource>,
    pub threads: Vec<ThreadSummary>,
    pub protection: ThreadProtectionStatus,
    pub scanned_at: Option<String>,
    pub scan_revision: String,
    pub total: u64,
    pub protected: u64,
    pub recoverable: u64,
    pub attention: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSettings {
    pub enabled: bool,
    pub interval_seconds: u32,
    pub protect_before_configuration_change: bool,
    pub include_archived: bool,
}

impl Default for ThreadSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_seconds: 60,
            protect_before_configuration_change: true,
            include_archived: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadIndex {
    pub version: u32,
    pub settings: ThreadSettings,
    pub sources: Vec<ThreadSource>,
    pub threads: Vec<ThreadSummary>,
    pub protection: ThreadProtectionStatus,
    pub last_scan_at: Option<String>,
    pub scan_revision: String,
}

impl Default for ThreadIndex {
    fn default() -> Self {
        Self {
            version: 1,
            settings: ThreadSettings::default(),
            sources: Vec::new(),
            threads: Vec::new(),
            protection: ThreadProtectionStatus {
                state: "idle".into(),
                last_success_at: None,
                last_attempt_at: None,
                protected_count: 0,
                pending_count: 0,
                failed_count: 0,
                bytes_protected: 0,
                current_path: None,
                error: None,
            },
            last_scan_at: None,
            scan_revision: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRestorePreview {
    pub thread: ThreadSummary,
    pub snapshot_hash: String,
    pub target_path: String,
    pub target_exists: bool,
    pub target_hash: Option<String>,
    pub conflict: bool,
    pub expected_hash: String,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ThreadListQuery {
    pub search: String,
    pub scope: String,
    pub status: String,
    pub source_id: Option<String>,
    pub offset: u32,
    pub limit: u32,
}

impl Default for ThreadListQuery {
    fn default() -> Self {
        Self { search: String::new(), scope: "all".into(), status: "all".into(), source_id: None, offset: 0, limit: 50 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadPage {
    pub threads: Vec<ThreadSummary>,
    pub total: u64,
    pub offset: u32,
    pub limit: u32,
    pub scan_revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReconcileResult {
    pub source_id: String,
    pub active_count: u64,
    pub archived_count: u64,
    pub completed_at: String,
    pub message: String,
}
