//! Stable IPC surface. Commands translate transport arguments into service calls.
pub(super) mod configuration;
pub(super) mod diagnostics;
pub(super) mod profiles;

use super::{
    dashboard::{self, Dashboard},
    desktop,
    state::AppState,
};
use crate::core::{self, Settings};
use tauri::State;

#[tauri::command]
pub(crate) async fn get_dashboard(state: State<'_, AppState>) -> Result<Dashboard, String> {
    dashboard::snapshot(&state).await
}

#[tauri::command]
pub(crate) async fn save_settings(
    state: State<'_, AppState>,
    input: Settings,
) -> Result<Settings, String> {
    core::save_settings(&state.paths, input)
}

#[tauri::command]
pub(crate) async fn open_codex() -> Result<String, String> {
    desktop::open_codex().await
}
