//! Thread protection is independent from connection credentials and routing.
mod client;
mod reconcile;
mod scan;
mod storage;
pub mod types;

use crate::core::AppPaths;
use chrono::Utc;
use std::sync::{Arc, Mutex};
#[cfg(not(test))]
use tauri::Manager;
use types::*;

#[derive(Clone)]
pub(crate) struct ThreadState {
    paths: AppPaths,
    runtime: Arc<Mutex<Runtime>>,
    operation: Arc<Mutex<()>>,
    initialized: Arc<Mutex<Option<Result<(), String>>>>,
}

#[derive(Default)]
struct Runtime {
    scanning: bool,
    last_attempt: Option<std::time::Instant>,
    error: Option<String>,
}

impl ThreadState {
    pub(crate) fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            runtime: Arc::new(Mutex::new(Runtime::default())),
            operation: Arc::new(Mutex::new(())),
            initialized: Arc::new(Mutex::new(None)),
        }
    }

    // Startup reconciliation runs on the same blocking pool as scans. Managing
    // the state after single-instance arbitration never delays window creation.
    fn initialize(&self) -> Result<(), String> {
        let mut initialized = self
            .initialized
            .lock()
            .map_err(|_| "线程保护初始化状态不可用。")?;
        initialized
            .get_or_insert_with(|| storage::recover(&self.paths))
            .clone()
    }

    pub(crate) fn dashboard(&self) -> Result<ThreadDashboard, String> {
        self.initialize()?;
        let mut dashboard = storage::overview(&self.paths)?;
        let runtime = self.runtime.lock().map_err(|_| "线程保护状态不可用。")?;
        if let Some(error) = &runtime.error {
            dashboard.protection.error = Some(error.clone());
            dashboard.error = Some(error.clone());
        }
        if runtime.scanning {
            dashboard.protection.state = "scanning".into();
        }
        Ok(dashboard)
    }

    pub(crate) fn list(&self, query: ThreadListQuery) -> Result<ThreadPage, String> {
        self.initialize()?;
        storage::page(&self.paths, &query)
    }

    pub(crate) fn rebuild(&self) -> Result<ThreadDashboard, String> {
        let _operation = self
            .operation
            .lock()
            .map_err(|_| "线程清单重建任务不可用。")?;
        storage::rebuild(&self.paths)?;
        *self
            .initialized
            .lock()
            .map_err(|_| "线程保护初始化状态不可用。")? = Some(Ok(()));
        self.scan_locked()
    }

    pub(crate) fn detail(&self, key: String) -> Result<ThreadDetail, String> {
        let thread = self.find(&key)?;
        scan::detail(&self.paths, &thread)
    }

    fn find(&self, key: &str) -> Result<ThreadSummary, String> {
        self.initialize()?;
        storage::lookup(&self.paths, key)?.ok_or_else(|| "线程记录不存在，请重新扫描。".into())
    }

    /// IPC dispatches filesystem work on the blocking pool, never the UI loop.
    pub(crate) fn start_scan(&self) -> Result<ThreadDashboard, String> {
        self.initialize()?;
        let _operation = match self.operation.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => return self.dashboard(),
            Err(_) => return Err("线程保护任务不可用，请重新打开 AhaX。".into()),
        };
        self.scan_locked()
    }

    fn scan_locked(&self) -> Result<ThreadDashboard, String> {
        self.scan_locked_mode(true, false)
    }

    fn scan_locked_mode(&self, full: bool, checkpoint: bool) -> Result<ThreadDashboard, String> {
        {
            let mut runtime = self.runtime.lock().map_err(|_| "线程保护状态不可用。")?;
            runtime.scanning = true;
            runtime.last_attempt = Some(std::time::Instant::now());
            runtime.error = None;
        }
        let scan = if checkpoint {
            scan::checkpoint(&self.paths)
        } else if full {
            scan::run(&self.paths)
        } else {
            scan::run_incremental(&self.paths)
        };
        let result = scan.and_then(|index| {
            storage::write(&self.paths, &index)?;
            Ok(make_dashboard(index))
        });
        let mut runtime = self.runtime.lock().map_err(|_| "线程保护状态不可用。")?;
        runtime.scanning = false;
        if let Err(error) = &result {
            runtime.error = Some(error.clone());
        }
        result
    }

    pub(crate) fn preview_restore(&self, key: String) -> Result<ThreadRestorePreview, String> {
        storage::preview_restore(&self.paths, &self.find(&key)?)
    }

    pub(crate) fn native_link(&self, key: String) -> Result<String, String> {
        let thread = self.find(&key)?;
        let id = uuid::Uuid::parse_str(&thread.thread_id)
            .map_err(|_| "此记录没有有效的原生线程 ID。")?;
        let current =
            std::fs::canonicalize(self.paths.config.parent().ok_or("当前线程目录不可用。")?)
                .map_err(|_| "当前线程目录不可读取。")?;
        let index = storage::read(&self.paths)?;
        let source = index
            .sources
            .iter()
            .find(|source| source.id == thread.source_id)
            .ok_or("线程来源不存在。")?;
        let source_root = std::fs::canonicalize(&source.root).map_err(|_| "线程来源暂不可用。")?;
        if current != source_root {
            return Err("此记录属于另一个数据目录，请在使用该目录的客户端中打开。".into());
        }
        let raw =
            std::fs::canonicalize(&thread.path).map_err(|_| "原始文件不存在，请先恢复线程。")?;
        if !raw.starts_with(&current) || !matches!(thread.integrity.as_str(), "valid" | "healthy") {
            return Err("此线程尚未通过完整性检查，请先重新扫描。".into());
        }
        Ok(format!("codex://threads/{id}"))
    }

    pub(crate) fn restore(
        &self,
        key: String,
        expected_hash: String,
    ) -> Result<ThreadDashboard, String> {
        let _operation = self.operation.lock().map_err(|_| "线程恢复任务不可用。")?;
        let thread = self.find(&key)?;
        let previous_dashboard = storage::overview(&self.paths)?;
        storage::apply_restore(&self.paths, &thread, &expected_hash)?;
        // A verified file publication is a completed restore. A later catalog
        // refresh failure must not turn that durable result into a false failure.
        match self.scan_locked_mode(false, false) {
            Ok(dashboard) => Ok(dashboard),
            Err(error) => {
                let warning = format!("线程文件已恢复并通过校验，但清单刷新未完成：{error}");
                let mut dashboard = storage::overview(&self.paths).unwrap_or(previous_dashboard);
                dashboard.protection.state = "attention".into();
                dashboard.protection.error = Some(warning.clone());
                dashboard.error = Some(warning.clone());
                if let Ok(mut runtime) = self.runtime.lock() {
                    runtime.error = Some(warning);
                }
                Ok(dashboard)
            }
        }
    }

    pub(crate) fn settings(&self) -> Result<ThreadSettings, String> {
        self.initialize()?;
        Ok(storage::read_state(&self.paths)?.settings)
    }

    pub(crate) fn reconcile(&self, source_id: String) -> Result<ThreadReconcileResult, String> {
        self.initialize()?;
        let _operation = self.operation.lock().map_err(|_| "线程索引任务不可用。")?;
        let dashboard = self.scan_locked()?;
        let source = dashboard
            .sources
            .iter()
            .find(|source| source.id == source_id)
            .ok_or("线程来源不存在，请重新扫描。")?;
        if !source.available || source.error.is_some() {
            return Err("线程来源不可读取，请先恢复目录访问。".into());
        }
        if dashboard
            .threads
            .iter()
            .filter(|thread| thread.source_id == source_id)
            .any(|thread| {
                thread.snapshot != SnapshotState::Protected
                    || !matches!(thread.integrity.as_str(), "healthy" | "valid" | "missing")
            })
        {
            return Err("请先完成此来源的线程保护，再重新索引。写入中的线程请稍后重试。".into());
        }
        let mut result = reconcile::run(&self.paths, source)?;
        if let Err(error) = self.scan_locked_mode(false, false) {
            result
                .message
                .push_str(&format!(" AhaX 清单暂未刷新：{error}"));
        }
        Ok(result)
    }

    pub(crate) fn save_settings(
        &self,
        settings: ThreadSettings,
    ) -> Result<ThreadDashboard, String> {
        self.initialize()?;
        if !(15..=3600).contains(&settings.interval_seconds) {
            return Err("线程保护间隔需为 15–3600 秒。".into());
        }
        let _operation = self.operation.lock().map_err(|_| "线程设置不可用。")?;
        let mut index = storage::read(&self.paths)?;
        index.settings = settings;
        storage::write(&self.paths, &index)?;
        Ok(make_dashboard(index))
    }

    pub(crate) fn checkpoint(&self) -> Result<(), String> {
        if !self.settings()?.protect_before_configuration_change {
            return Ok(());
        }
        let _operation = self.operation.lock().map_err(|_| "线程保护任务不可用。")?;
        let result = self.scan_locked_mode(false, true)?;
        if result.protection.failed_count > 0
            || result.sources.iter().any(|source| source.error.is_some())
        {
            return Err("线程保护检查未完成，请在线程页面处理保护错误后重试。".into());
        }
        Ok(())
    }

    pub(crate) fn tick(&self) {
        if self.initialize().is_err() {
            return;
        }
        let Ok(index) = storage::read_state(&self.paths) else {
            return;
        };
        if !index.settings.enabled {
            return;
        }
        if let Ok(runtime) = self.runtime.lock() {
            if runtime.scanning
                || runtime.last_attempt.is_some_and(|last| {
                    last.elapsed().as_secs() < u64::from(index.settings.interval_seconds)
                })
            {
                return;
            }
        }
        let due = index
            .last_scan_at
            .as_deref()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .map(|at| {
                (Utc::now() - at.with_timezone(&Utc)).num_seconds()
                    >= i64::from(index.settings.interval_seconds)
            })
            .unwrap_or(true);
        if due {
            if let Ok(_operation) = self.operation.try_lock() {
                let _ = self.scan_locked_mode(false, false);
            }
        }
    }
}

fn make_dashboard(index: ThreadIndex) -> ThreadDashboard {
    let total = index.threads.len() as u64;
    let protected = index
        .threads
        .iter()
        .filter(|thread| thread.snapshot == SnapshotState::Protected)
        .count() as u64;
    let recoverable = index
        .threads
        .iter()
        .filter(|thread| thread.recoverability == "recoverable")
        .count() as u64;
    let attention = index
        .threads
        .iter()
        .filter(|thread| {
            !matches!(thread.integrity.as_str(), "healthy" | "valid")
                || thread.snapshot != SnapshotState::Protected
                || thread.state_index.as_deref() == Some("missing")
        })
        .count() as u64;
    ThreadDashboard {
        sources: index.sources,
        threads: index.threads,
        error: index.protection.error.clone(),
        protection: index.protection,
        scanned_at: index.last_scan_at,
        scan_revision: index.scan_revision,
        total,
        protected,
        recoverable,
        attention,
    }
}

#[cfg(not(test))]
pub(crate) fn start_scheduler(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let state = handle.state::<ThreadState>().inner().clone();
            let _ = tauri::async_runtime::spawn_blocking(move || state.tick()).await;
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    fn state() -> (tempfile::TempDir, ThreadState) {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: directory.path().join("data"),
            config: directory.path().join("new-user-home/config.toml"),
            helper: directory.path().join("unused.exe"),
        };
        (directory, ThreadState::new(paths))
    }

    #[test]
    fn first_configuration_checkpoint_accepts_a_new_user_without_creating_source_files() {
        let (_directory, state) = state();
        state.checkpoint().unwrap();
        assert!(!state.paths.config.parent().unwrap().exists());
        assert_eq!(state.dashboard().unwrap().total, 0);
    }

    #[test]
    fn paused_timer_does_not_scan_and_custom_intervals_are_persisted() {
        let (_directory, state) = state();
        let settings = ThreadSettings {
            enabled: false,
            interval_seconds: 125,
            ..ThreadSettings::default()
        };
        state.save_settings(settings).unwrap();
        state.tick();
        assert!(state.dashboard().unwrap().scanned_at.is_none());
        assert_eq!(state.settings().unwrap().interval_seconds, 125);
        assert!(state
            .save_settings(ThreadSettings {
                interval_seconds: 0,
                ..ThreadSettings::default()
            })
            .is_err());
    }

    #[test]
    fn concurrent_scan_reads_progress_without_locking_runtime_twice() {
        let (_directory, state) = state();
        state.initialize().unwrap();
        let _operation = state.operation.lock().unwrap();
        state.runtime.lock().unwrap().scanning = true;
        assert_eq!(state.start_scan().unwrap().protection.state, "scanning");
    }

    #[test]
    fn native_links_require_registered_existing_threads() {
        let (_directory, state) = state();
        assert!(state.native_link("../../outside".into()).is_err());
    }

    #[test]
    fn completed_restore_reports_catalog_failure_as_a_warning() {
        use fs2::FileExt;
        let (_directory, state) = state();
        let root = state.paths.config.parent().unwrap();
        let folder = root.join("sessions/2026/10/04");
        std::fs::create_dir_all(&folder).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let path = folder.join(format!("rollout-2026-10-04T00-00-00-{id}.jsonl"));
        let mut bytes =
            serde_json::to_vec(&serde_json::json!({"type":"session_meta","payload":{"id":id}}))
                .unwrap();
        bytes.push(b'\n');
        std::fs::write(&path, &bytes).unwrap();
        let key = state.start_scan().unwrap().threads[0].key.clone();
        std::fs::remove_file(&path).unwrap();
        state.start_scan().unwrap();
        let preview = state.preview_restore(key.clone()).unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(state.paths.data.join("threads/catalog.lock"))
            .unwrap();
        lock.lock_exclusive().unwrap();
        let result = state.restore(key, preview.expected_hash).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(result.error.unwrap().contains("已恢复"));
        assert!(state.dashboard().unwrap().error.unwrap().contains("已恢复"));
    }
}
