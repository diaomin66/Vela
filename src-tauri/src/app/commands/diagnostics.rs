//! Diagnostic IPC adapters; cancellation uses the same registry as metadata requests.
use super::super::{
    inspection,
    state::{self, AppState},
};
use crate::diagnostics;
use tauri::State;

#[tauri::command]
pub(crate) async fn validate_profile(
    state: State<'_, AppState>,
    id: String,
    run_id: Option<String>,
    model_id: Option<String>,
) -> Result<diagnostics::ValidationResult, String> {
    inspection::validate_profile(&state, id, run_id, model_id).await
}

#[tauri::command]
pub(crate) async fn run_diagnostics(
    state: State<'_, AppState>,
    run_id: Option<String>,
    include_network: Option<bool>,
) -> Result<diagnostics::DiagnosticReport, String> {
    inspection::run_diagnostics(&state, run_id, include_network).await
}

#[tauri::command]
pub(crate) fn cancel_diagnostics(state: State<'_, AppState>, run_id: String) -> Result<(), String> {
    state::cancel_run(&state, run_id)
}
