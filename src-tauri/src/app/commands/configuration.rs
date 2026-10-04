//! Configuration IPC adapters preserve preview-before-apply conflict checks.
use super::super::{
    runtime::{self, refresh_gateway},
    state::AppState,
};
use crate::{
    catalog,
    core::{self, Backup, ChangePreview},
};
use tauri::State;

async fn checkpoint(threads: &crate::threads::ThreadState) -> Result<(), String> {
    let threads = threads.clone();
    tauri::async_runtime::spawn_blocking(move || threads.checkpoint()).await
        .map_err(|_| "线程保护检查意外中断，未修改配置。".to_string())?
}

#[tauri::command]
pub(crate) fn preview_profile(
    state: State<'_, AppState>,
    id: String,
) -> Result<ChangePreview, String> {
    core::preview_profile(&state.paths, &id)
}
#[tauri::command]
pub(crate) async fn apply_profile(
    state: State<'_, AppState>,
    threads: State<'_, crate::threads::ThreadState>,
    id: String,
    expected_hash: String,
) -> Result<Backup, String> {
    checkpoint(&threads).await?;
    core::apply_profile(&state.paths, &id, &expected_hash)
}
#[tauri::command]
pub(crate) fn preview_restore(
    state: State<'_, AppState>,
    id: String,
) -> Result<ChangePreview, String> {
    core::preview_restore(&state.paths, &id)
}
#[tauri::command]
pub(crate) async fn restore_backup(
    state: State<'_, AppState>,
    threads: State<'_, crate::threads::ThreadState>,
    id: String,
    expected_hash: String,
) -> Result<Backup, String> {
    checkpoint(&threads).await?;
    let result = core::restore_backup(&state.paths, &id, &expected_hash)?;
    refresh_gateway(&state).await?;
    Ok(result)
}
#[tauri::command]
pub(crate) fn preview_repair(state: State<'_, AppState>) -> Result<ChangePreview, String> {
    core::preview_repair(&state.paths)
}
#[tauri::command]
pub(crate) async fn apply_repair(
    state: State<'_, AppState>,
    threads: State<'_, crate::threads::ThreadState>,
    expected_hash: String,
) -> Result<Backup, String> {
    checkpoint(&threads).await?;
    let result = core::apply_repair(&state.paths, &expected_hash)?;
    refresh_gateway(&state).await?;
    Ok(result)
}
#[tauri::command]
pub(crate) fn preview_gateway(
    state: State<'_, AppState>,
    default_route_id: Option<String>,
) -> Result<ChangePreview, String> {
    catalog::preview(&state.paths, default_route_id.as_deref())
}

#[tauri::command]
pub(crate) async fn apply_gateway(
    state: State<'_, AppState>,
    threads: State<'_, crate::threads::ThreadState>,
    default_route_id: Option<String>,
    expected_hash: String,
) -> Result<Backup, String> {
    checkpoint(&threads).await?;
    runtime::apply_gateway(&state, default_route_id, expected_hash).await
}
