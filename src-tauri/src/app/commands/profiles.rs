//! Channel IPC adapters. Persistence and network work live in their owning services.
use super::super::{
    metadata::synchronize,
    runtime::refresh_gateway,
    state::{register_run, AppState},
};
use crate::core::{self, Profile, ProfileInput};
use tauri::State;

#[tauri::command]
pub(crate) async fn save_profile(
    state: State<'_, AppState>,
    input: ProfileInput,
) -> Result<Profile, String> {
    let profile = core::save_profile(&state.paths, input)?;
    refresh_gateway(&state).await?;
    Ok(profile)
}
#[tauri::command]
pub(crate) async fn delete_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    core::delete_profile(&state.paths, &id)?;
    refresh_gateway(&state).await
}
#[tauri::command]
pub(crate) async fn sync_profile(
    state: State<'_, AppState>,
    id: String,
    run_id: Option<String>,
) -> Result<Profile, String> {
    let (_run, receiver) = register_run(&state, run_id)?;
    synchronize(&state, &id, receiver).await
}
