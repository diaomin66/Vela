//! Bounded evaluation files, separate from connection/configuration storage.
use super::types::*;
use crate::core::{self, AppPaths};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

pub(super) const HISTORY_LIMIT: usize = 100;
const RECORDS_PER_RUN: usize = 18;
const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RUN_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EvaluationStore {
    #[serde(default)]
    pub plan: EvaluationPlan,
    #[serde(default)]
    pub next_run_at: Option<String>,
    #[serde(default)]
    pub active_run_id: Option<String>,
    #[serde(default)]
    pub history: Vec<RunSummary>,
    #[serde(default)]
    pub records_version: u32,
    #[serde(default)]
    pub records: Vec<EvaluationRecord>,
    #[serde(default)]
    pub error: Option<String>,
}

impl Default for EvaluationStore {
    fn default() -> Self {
        Self {
            plan: EvaluationPlan::default(),
            next_run_at: None,
            active_run_id: None,
            history: Vec::new(),
            records_version: 1,
            records: Vec::new(),
            error: None,
        }
    }
}

pub(super) fn directory(paths: &AppPaths) -> PathBuf {
    paths.evaluations_directory()
}
fn read_bounded(path: &std::path::Path, limit: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|_| "无法读取评测记录。")?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("评测记录格式异常或超过大小限制。".into());
    }
    fs::read(path).map_err(|_| "无法读取评测记录。".into())
}
pub(super) fn read(paths: &AppPaths) -> Result<EvaluationStore, String> {
    let path = directory(paths).join("index.json");
    if !path.exists() {
        return Ok(EvaluationStore::default());
    }
    let mut store: EvaluationStore = serde_json::from_slice(&read_bounded(&path, MAX_INDEX_BYTES)?)
        .map_err(|_| "评测计划文件无法解析，请保留文件后检查。")?;
    store.history.truncate(HISTORY_LIMIT);
    store.records.truncate(HISTORY_LIMIT * RECORDS_PER_RUN);
    Ok(store)
}
pub(super) fn write(paths: &AppPaths, store: &EvaluationStore) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(store).map_err(|_| "无法保存评测计划。")?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err("评测索引超过保存限制。".into());
    }
    core::atomic_write(&directory(paths).join("index.json"), &bytes)
}
pub(super) fn run_path(paths: &AppPaths, id: &str) -> Result<PathBuf, String> {
    core::validate_id(id)?;
    Ok(directory(paths).join("runs").join(format!("{id}.json")))
}
pub(super) fn read_run(paths: &AppPaths, id: &str) -> Result<EvaluationRun, String> {
    serde_json::from_slice(&read_bounded(&run_path(paths, id)?, MAX_RUN_BYTES)?)
        .map_err(|_| "评测记录无法解析。".into())
}
pub(super) fn write_run(paths: &AppPaths, run: &EvaluationRun) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(run).map_err(|_| "无法保存评测结果。")?;
    if bytes.len() as u64 > MAX_RUN_BYTES {
        return Err("评测结果超过保存限制。".into());
    }
    core::atomic_write(&run_path(paths, &run.id)?, &bytes)
}
pub(super) fn register(paths: &AppPaths, run: &EvaluationRun) -> Result<(), String> {
    let _lock = paths.lock()?;
    let mut store = read(paths)?;
    write_run(paths, run)?;
    store.active_run_id = Some(run.id.clone());
    store.error = None;
    write(paths, &store)
}
pub(super) fn finish(paths: &AppPaths, run: &EvaluationRun) -> Result<(), String> {
    let _lock = paths.lock()?;
    let mut store = read(paths)?;
    write_run(paths, run)?;
    store.history.retain(|item| item.id != run.id);
    store.history.insert(0, RunSummary::from(run));
    let expired = store
        .history
        .split_off(store.history.len().min(HISTORY_LIMIT));
    store.records.retain(|record| {
        record.run_id != run.id
            && store
                .history
                .iter()
                .any(|summary| summary.id == record.run_id)
    });
    store.records.splice(
        0..0,
        run.results
            .iter()
            .take(RECORDS_PER_RUN)
            .map(|result| EvaluationRecord::from_result(run, result)),
    );
    store.records.truncate(HISTORY_LIMIT * RECORDS_PER_RUN);
    if store.active_run_id.as_deref() == Some(&run.id) {
        store.active_run_id = None;
    }
    write(paths, &store)?;
    for old in expired {
        if let Ok(path) = run_path(paths, &old.id) {
            let _ = fs::remove_file(path);
        }
    }
    // Explicit exports belong to the user and outlive the rolling app history.
    Ok(())
}
pub(super) fn recover(paths: &AppPaths) -> Result<(), String> {
    super::deletion::recover(paths)?;
    migrate_records(paths)?;
    let store = read(paths)?;
    if let Some(id) = store.active_run_id {
        let mut run = read_run(paths, &id)?;
        if run.status == RunStatus::Running {
            run.status = RunStatus::Interrupted;
            run.finished_at = Some(chrono::Utc::now().to_rfc3339());
            run.error = Some("上次评测因程序退出而中断；不会自动重发已产生费用的请求。".into());
        }
        finish(paths, &run)?;
    }
    Ok(())
}

fn migrate_records(paths: &AppPaths) -> Result<(), String> {
    let _lock = paths.lock()?;
    let mut store = read(paths)?;
    if store.records_version >= 1 {
        return Ok(());
    }
    store.records.clear();
    let mut unavailable = false;
    for summary in &store.history {
        match read_run(paths, &summary.id) {
            Ok(run) => store.records.extend(
                run.results
                    .iter()
                    .take(RECORDS_PER_RUN)
                    .map(|result| EvaluationRecord::from_result(&run, result)),
            ),
            Err(_) => unavailable = true,
        }
    }
    if unavailable {
        store.error = Some("部分旧评测记录无法读取，其余历史已保留。".into());
    }
    store.records_version = 1;
    write(paths, &store)
}
pub(super) fn export(paths: &AppPaths, id: &str) -> Result<EvaluationExport, String> {
    let run = read_run(paths, id)?;
    let content = serde_json::to_string_pretty(&run).map_err(|_| "无法导出评测记录。")?;
    let file_name = format!("ahaX-evaluation-{id}.json");
    let target = paths.exports_directory().join(&file_name);
    core::atomic_write(&target, content.as_bytes())?;
    Ok(EvaluationExport {
        file_name,
        content,
        path: target.to_string_lossy().into(),
    })
}
