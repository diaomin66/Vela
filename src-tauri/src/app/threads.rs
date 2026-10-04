//! Thin command adapters. Filesystem and encryption work stays off the UI loop.
use crate::threads::{types::*, ThreadState};
use tauri::State;

async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| "线程任务意外中断，请重试。".to_string())?
}

#[tauri::command]
pub(super) async fn preview_thread_deletion(state: State<'_, ThreadState>, keys: Vec<String>) -> Result<ThreadDeletionPreview, String> {
    let state = state.inner().clone();
    blocking(move || state.preview_delete(keys)).await
}

#[tauri::command]
pub(super) async fn delete_threads(state: State<'_, ThreadState>, keys: Vec<String>, expected_hash: String) -> Result<ThreadDeletionResult, String> {
    let state = state.inner().clone();
    blocking(move || state.delete(keys, expected_hash)).await
}

#[tauri::command]
pub(super) async fn list_thread_trash(state: State<'_, ThreadState>, offset: u32, limit: u32) -> Result<ThreadTrashPage, String> {
    let state = state.inner().clone();
    blocking(move || state.list_trash(offset, limit)).await
}

#[tauri::command]
pub(super) async fn preview_thread_trash_restore(state: State<'_, ThreadState>, id: String) -> Result<ThreadTrashRestorePreview, String> {
    let state = state.inner().clone();
    blocking(move || state.preview_trash_restore(id)).await
}

#[tauri::command]
pub(super) async fn restore_thread_trash(state: State<'_, ThreadState>, id: String, expected_hash: String) -> Result<ThreadDeletionResult, String> {
    let state = state.inner().clone();
    blocking(move || state.restore_trash(id, expected_hash)).await
}

fn compact(mut dashboard: ThreadDashboard) -> ThreadDashboard {
    dashboard.threads.clear();
    dashboard
}

#[tauri::command]
pub(super) async fn get_thread_dashboard(
    state: State<'_, ThreadState>,
) -> Result<ThreadDashboard, String> {
    let state = state.inner().clone();
    blocking(move || state.dashboard()).await
}
#[tauri::command]
pub(super) async fn scan_threads(state: State<'_, ThreadState>) -> Result<ThreadDashboard, String> {
    let state = state.inner().clone();
    blocking(move || state.start_scan().map(compact)).await
}
#[tauri::command]
pub(super) async fn get_thread_detail(
    state: State<'_, ThreadState>,
    key: String,
) -> Result<ThreadDetail, String> {
    let state = state.inner().clone();
    blocking(move || state.detail(key)).await
}
#[tauri::command]
pub(super) async fn preview_thread_restore(
    state: State<'_, ThreadState>,
    key: String,
) -> Result<ThreadRestorePreview, String> {
    let state = state.inner().clone();
    blocking(move || state.preview_restore(key)).await
}
#[tauri::command]
pub(super) async fn restore_thread(
    state: State<'_, ThreadState>,
    key: String,
    expected_hash: String,
) -> Result<ThreadDashboard, String> {
    let state = state.inner().clone();
    blocking(move || state.restore(key, expected_hash).map(compact)).await
}
#[tauri::command]
pub(super) async fn get_thread_settings(
    state: State<'_, ThreadState>,
) -> Result<ThreadSettings, String> {
    let state = state.inner().clone();
    blocking(move || state.settings()).await
}
#[tauri::command]
pub(super) async fn save_thread_settings(
    state: State<'_, ThreadState>,
    settings: ThreadSettings,
) -> Result<ThreadDashboard, String> {
    let state = state.inner().clone();
    blocking(move || state.save_settings(settings).map(compact)).await
}

#[tauri::command]
pub(super) async fn reconcile_thread_index(
    state: State<'_, ThreadState>,
    source_id: String,
) -> Result<ThreadReconcileResult, String> {
    let state = state.inner().clone();
    blocking(move || state.reconcile(source_id)).await
}

#[tauri::command]
pub(super) async fn list_threads(
    state: State<'_, ThreadState>,
    query: ThreadListQuery,
) -> Result<ThreadPage, String> {
    let state = state.inner().clone();
    blocking(move || state.list(query)).await
}

#[tauri::command]
pub(super) async fn rebuild_thread_inventory(
    state: State<'_, ThreadState>,
) -> Result<ThreadDashboard, String> {
    let state = state.inner().clone();
    blocking(move || state.rebuild().map(compact)).await
}

#[tauri::command]
pub(super) async fn open_thread(
    state: State<'_, ThreadState>,
    key: String,
) -> Result<String, String> {
    let state = state.inner().clone();
    blocking(move || {
        let link = state.native_link(key)?;
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("explorer.exe")
                .arg(link)
                .creation_flags(0x08000000)
                .spawn()
                .map_err(|_| "无法打开原生线程，请确认官方客户端已安装。")?;
            Ok("已请求打开原生线程。客户端的当前项目与筛选设置可能影响显示。".into())
        }
        #[cfg(not(windows))]
        {
            let _ = link;
            Err("此版本仅支持 Windows 原生线程打开。".into())
        }
    })
    .await
}
