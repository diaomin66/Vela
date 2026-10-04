//! IPC adapters for restart-applied storage locations.
use super::state::AppState;
use crate::locations::{self, LocationPreferences, LocationPreview, LocationStatus};
use tauri::State;

async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| "存储位置任务意外中断，请重试。".to_string())?
}

pub(super) fn activate_pending(paths: crate::core::AppPaths) -> crate::core::AppPaths {
    locations::activate_pending(paths.clone()).unwrap_or(paths)
}

#[tauri::command]
pub(crate) async fn get_location_preferences(
    state: State<'_, AppState>,
) -> Result<LocationStatus, String> {
    let paths = state.paths.clone();
    blocking(move || locations::status(&paths)).await
}

#[tauri::command]
pub(crate) async fn preview_location_preferences(
    state: State<'_, AppState>,
    preferences: LocationPreferences,
) -> Result<LocationPreview, String> {
    let paths = state.paths.clone();
    blocking(move || locations::preview(&paths, preferences)).await
}

#[tauri::command]
pub(crate) async fn save_location_preferences(
    state: State<'_, AppState>,
    threads: State<'_, crate::threads::ThreadState>,
    preferences: LocationPreferences,
    expected_hash: String,
) -> Result<LocationStatus, String> {
    let paths = state.paths.clone();
    let threads = threads.inner().clone();
    blocking(move || {
        locations::save_with_checkpoint(&paths, preferences, &expected_hash, || {
            threads.checkpoint_location_change()
        })
    })
    .await
}
