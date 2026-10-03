//! Bounded evaluation files, separate from connection/configuration storage.
use super::types::*;
use crate::core::{self, AppPaths};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

const HISTORY_LIMIT: usize = 100;
const MAX_INDEX_BYTES: u64 = 256 * 1024;
const MAX_RUN_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Default, Serialize, Deserialize)]
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
    pub error: Option<String>,
}

fn directory(paths: &AppPaths) -> PathBuf {
    paths.data.join("evaluations")
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
    Ok(store)
}
pub(super) fn write(paths: &AppPaths, store: &EvaluationStore) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(store).map_err(|_| "无法保存评测计划。")?;
    core::atomic_write(&directory(paths).join("index.json"), &bytes)
}
fn run_path(paths: &AppPaths, id: &str) -> Result<PathBuf, String> {
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
pub(super) fn export(paths: &AppPaths, id: &str) -> Result<EvaluationExport, String> {
    let run = read_run(paths, id)?;
    let content = serde_json::to_string_pretty(&run).map_err(|_| "无法导出评测记录。")?;
    let file_name = format!("Vela-evaluation-{id}.json");
    let target = directory(paths).join("exports").join(&file_name);
    core::atomic_write(&target, content.as_bytes())?;
    Ok(EvaluationExport {
        file_name,
        content,
        path: target.to_string_lossy().into(),
    })
}
