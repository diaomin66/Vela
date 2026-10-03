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

#[tauri::command]
pub(crate) fn preview_profile(
    state: State<'_, AppState>,
    id: String,
) -> Result<ChangePreview, String> {
    core::preview_profile(&state.paths, &id)
}
#[tauri::command]
pub(crate) fn apply_profile(
    state: State<'_, AppState>,
    id: String,
    expected_hash: String,
) -> Result<Backup, String> {
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
    id: String,
    expected_hash: String,
) -> Result<Backup, String> {
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
    expected_hash: String,
) -> Result<Backup, String> {
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
    default_route_id: Option<String>,
    expected_hash: String,
) -> Result<Backup, String> {
    runtime::apply_gateway(&state, default_route_id, expected_hash).await
}
