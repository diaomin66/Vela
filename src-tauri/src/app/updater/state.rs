//! Update state and persisted preference policy, independent of the GUI runtime.
use crate::core::{self, AppPaths};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{fs, sync::Arc};

const PREFERENCES_FILE: &str = "update-preferences.json";

pub(super) struct UpdateMetadata {
    pub(super) version: String,
    pub(super) notes: Option<String>,
    pub(super) identity: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum UpdatePhase {
    Idle,
    Checking,
    Latest,
    Available,
    Downloading,
    Ready,
    Installing,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateStatus {
    pub(super) phase: UpdatePhase,
    pub(super) current_version: String,
    pub(super) version: Option<String>,
    pub(super) notes: Option<String>,
    pub(super) downloaded_bytes: u64,
    pub(super) total_bytes: Option<u64>,
    pub(super) checked_at: Option<String>,
    pub(super) error: Option<String>,
    pub(super) auto_download: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Preferences {
    pub(super) auto_download: bool,
}

pub(super) struct UpdateSession {
    pub(super) status: UpdateStatus,
    artifact_identity: Option<String>,
    // Bytes only enter this slot after the official updater verifies the signature.
    pub(super) verified_bytes: Option<Arc<Vec<u8>>>,
}

impl UpdateSession {
    pub(super) fn new(preferences: Result<Preferences, String>) -> Self {
        let (auto_download, error) = match preferences {
            Ok(preferences) => (preferences.auto_download, None),
            Err(error) => (false, Some(error)),
        };
        Self {
            status: UpdateStatus {
                phase: UpdatePhase::Idle,
                current_version: env!("CARGO_PKG_VERSION").into(),
                version: None,
                notes: None,
                downloaded_bytes: 0,
                total_bytes: None,
                checked_at: None,
                error,
                auto_download,
            },
            artifact_identity: None,
            verified_bytes: None,
        }
    }
    pub(super) fn busy(&self) -> bool {
        matches!(
            self.status.phase,
            UpdatePhase::Checking | UpdatePhase::Downloading | UpdatePhase::Installing
        )
    }
    pub(super) fn begin_check(&mut self) -> bool {
        if self.busy() {
            return false;
        }
        self.status.phase = UpdatePhase::Checking;
        self.status.error = None;
        true
    }
    pub(super) fn begin_download(&mut self) -> bool {
        if self.busy() || self.verified_bytes.is_some() || self.artifact_identity.is_none() {
            return false;
        }
        self.status.phase = UpdatePhase::Downloading;
        self.status.error = None;
        self.status.downloaded_bytes = 0;
        self.status.total_bytes = None;
        true
    }
    pub(super) fn fail(&mut self, message: String) {
        self.status.phase = if self.verified_bytes.is_some() {
            UpdatePhase::Ready
        } else {
            UpdatePhase::Error
        };
        self.status.error = Some(message);
    }
    pub(super) fn checked(&mut self, update: Option<UpdateMetadata>) -> bool {
        self.status.checked_at = Some(Utc::now().to_rfc3339());
        self.status.error = None;
        if let Some(update) = update {
            // A second check for the same signed artifact should not discard a ready download.
            let same_artifact = self.artifact_identity.as_ref() == Some(&update.identity);
            if !same_artifact {
                self.verified_bytes = None;
                self.status.downloaded_bytes = 0;
                self.status.total_bytes = None;
            }
            self.status.version = Some(update.version.clone());
            self.status.notes = update.notes;
            self.status.phase = if self.verified_bytes.is_some() {
                UpdatePhase::Ready
            } else {
                UpdatePhase::Available
            };
            self.artifact_identity = Some(update.identity);
            self.status.auto_download && self.verified_bytes.is_none()
        } else {
            self.status.phase = UpdatePhase::Latest;
            self.status.version = None;
            self.status.notes = None;
            self.status.downloaded_bytes = 0;
            self.status.total_bytes = None;
            self.artifact_identity = None;
            self.verified_bytes = None;
            false
        }
    }
    pub(super) fn downloaded(&mut self, bytes: Vec<u8>) {
        self.status.downloaded_bytes = bytes.len() as u64;
        self.status.total_bytes = Some(bytes.len() as u64);
        self.verified_bytes = Some(Arc::new(bytes));
        self.status.phase = UpdatePhase::Ready;
        self.status.error = None;
    }
}

pub(super) fn read_preferences(paths: &AppPaths) -> Result<Preferences, String> {
    match fs::read(paths.data.join(PREFERENCES_FILE)) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "自动下载偏好无法读取，请重新设置。".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Preferences {
            auto_download: true,
        }),
        Err(_) => Err("无法读取更新偏好，请检查应用数据目录权限。".into()),
    }
}

pub(super) fn write_preferences(paths: &AppPaths, auto_download: bool) -> Result<(), String> {
    let _lock = paths.lock()?;
    let bytes =
        serde_json::to_vec(&Preferences { auto_download }).map_err(|_| "无法保存更新偏好。")?;
    core::atomic_write(&paths.data.join(PREFERENCES_FILE), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> UpdateSession {
        UpdateSession::new(Ok(Preferences {
            auto_download: true,
        }))
    }
    fn release(identity: &str) -> UpdateMetadata {
        UpdateMetadata {
            version: "99.0.0".into(),
            notes: Some("Release notes".into()),
            identity: identity.into(),
        }
    }
    fn paths(directory: &std::path::Path) -> AppPaths {
        AppPaths {
            data: directory.join("data"),
            config: directory.join("unused-config"),
            helper: directory.join("unused-helper"),
            locations: None,
        }
    }

    #[test]
    fn checks_are_single_flight_and_failures_can_be_retried() {
        let mut state = session();
        assert!(state.begin_check());
        assert!(!state.begin_check());
        assert!(!state.begin_download());
        state.fail("offline".into());
        assert_eq!(state.status.phase, UpdatePhase::Error);
        assert!(state.begin_check());
        assert!(state.status.error.is_none());
    }

    #[test]
    fn network_failure_keeps_a_verified_update_ready_for_installation() {
        let mut state = session();
        state.downloaded(vec![1, 2, 3]);
        assert!(state.begin_check());
        state.fail("offline".into());
        assert_eq!(state.status.phase, UpdatePhase::Ready);
        assert_eq!(
            state.verified_bytes.as_ref().unwrap().as_slice(),
            &[1, 2, 3]
        );
        assert_eq!(state.status.downloaded_bytes, 3);
    }

    #[test]
    fn installation_and_download_block_background_checks() {
        let mut state = session();
        for phase in [UpdatePhase::Downloading, UpdatePhase::Installing] {
            state.status.phase = phase;
            assert!(!state.begin_check());
            assert!(!state.begin_download());
            assert_eq!(state.status.phase, phase);
        }
    }

    #[test]
    fn same_signed_artifact_reuses_bytes_but_changed_signature_requires_verification() {
        let mut state = session();
        assert!(state.checked(Some(release("signed-a"))));
        state.downloaded(vec![1, 2, 3]);
        assert!(!state.checked(Some(release("signed-a"))));
        assert_eq!(state.status.phase, UpdatePhase::Ready);
        assert!(state.verified_bytes.is_some());
        assert!(state.checked(Some(release("signed-b"))));
        assert_eq!(state.status.phase, UpdatePhase::Available);
        assert!(state.verified_bytes.is_none());
        assert_eq!(state.status.downloaded_bytes, 0);
    }

    #[test]
    fn failed_download_can_retry_without_retaining_unverified_bytes() {
        let mut state = session();
        assert!(!state.begin_download());
        state.checked(Some(release("signed-a")));
        assert!(state.begin_download());
        state.status.downloaded_bytes = 123;
        state.fail("signature rejected".into());
        assert!(state.verified_bytes.is_none());
        assert!(state.begin_download());
        assert_eq!(state.status.downloaded_bytes, 0);
        assert!(state.status.error.is_none());
        assert_eq!(state.status.phase, UpdatePhase::Downloading);
    }

    #[test]
    fn manual_download_remains_available_when_automatic_download_is_disabled() {
        let mut state = session();
        state.status.auto_download = false;
        assert!(!state.checked(Some(release("signed-a"))));
        assert_eq!(state.status.phase, UpdatePhase::Available);
        assert!(state.begin_download());
    }

    #[test]
    fn latest_check_removes_old_artifact_and_pending_metadata() {
        let mut state = session();
        state.status.version = Some("99.0.0".into());
        state.status.notes = Some("old notes".into());
        state.downloaded(vec![1, 2, 3]);
        assert!(!state.checked(None));
        assert_eq!(state.status.phase, UpdatePhase::Latest);
        assert!(state.status.checked_at.is_some());
        assert!(state.status.version.is_none());
        assert!(state.status.notes.is_none());
        assert!(state.verified_bytes.is_none());
        assert_eq!(state.status.downloaded_bytes, 0);
    }

    #[test]
    fn disabling_auto_download_survives_restart_and_is_isolated() {
        let directory = tempfile::tempdir().unwrap();
        let a = paths(&directory.path().join("a"));
        let b = paths(&directory.path().join("b"));
        assert!(read_preferences(&a).unwrap().auto_download);
        write_preferences(&a, false).unwrap();
        assert!(!read_preferences(&a).unwrap().auto_download);
        assert!(read_preferences(&b).unwrap().auto_download);
        assert!(!a.config.exists());
        assert!(!a.data.join("connections.json").exists());
    }

    #[test]
    fn corrupt_preferences_do_not_silently_reenable_automatic_downloads() {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(directory.path());
        fs::create_dir_all(&paths.data).unwrap();
        fs::write(paths.data.join(PREFERENCES_FILE), "broken").unwrap();
        let state = UpdateSession::new(read_preferences(&paths));
        assert!(!state.status.auto_download);
        assert!(state.status.error.is_some());
        write_preferences(&paths, true).unwrap();
        assert!(read_preferences(&paths).unwrap().auto_download);
    }
}
