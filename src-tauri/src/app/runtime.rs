//! Gateway lifecycle and transactional switching to an applied model catalog.
use super::state::AppState;
use crate::{
    catalog,
    core::{self, AppPaths, Backup},
    gateway, security,
};

fn remember_status<T>(state: &AppState, result: Result<T, String>) -> Result<T, String> {
    if let Ok(mut error) = state.gateway_error.lock() {
        *error = result.as_ref().err().cloned();
    }
    result
}

pub(super) async fn refresh_gateway(state: &AppState) -> Result<(), String> {
    let store = core::load_store(&state.paths)?;
    let port = effective_gateway_port(&state.paths, &store);
    let mut current = state.gateway.lock().await;
    if let Some(handle) = current.as_ref() {
        if handle.status().running && handle.status().port == port {
            return remember_status(
                state,
                handle.update_catalog(catalog::runtime_catalog(&store)),
            );
        }
    }
    let token = security::gateway_token(&state.paths)?;
    match gateway::start(catalog::runtime_catalog(&store), token, port).await {
        Ok(handle) => {
            if let Some(mut previous) = current.take() {
                previous.shutdown().await;
            }
            *current = Some(handle);
            remember_status(state, Ok(()))
        }
        Err(error) => remember_status(state, Err(error)),
    }
}
pub(super) fn effective_gateway_port(paths: &AppPaths, store: &core::Store) -> u16 {
    core::read_config(paths)
        .ok()
        .as_ref()
        .and_then(|bytes| core::config_text(bytes).ok())
        .and_then(catalog::configured_port)
        .unwrap_or(store.settings.gateway_port)
}

pub(super) async fn apply_gateway(
    state: &AppState,
    default_route_id: Option<String>,
    expected_hash: String,
) -> Result<Backup, String> {
    let store = core::load_store(&state.paths)?;
    let mut current = state.gateway.lock().await;
    let mut replacement = if current.as_ref().is_some_and(|handle| {
        handle.status().running && handle.status().port == store.settings.gateway_port
    }) {
        None
    } else {
        Some(remember_status(
            state,
            gateway::start(
                catalog::runtime_catalog(&store),
                security::gateway_token(&state.paths)?,
                store.settings.gateway_port,
            )
            .await,
        )?)
    };
    match catalog::apply(&state.paths, default_route_id.as_deref(), &expected_hash) {
        Ok(backup) => {
            if let Some(handle) = replacement.take() {
                if let Some(mut previous) = current.take() {
                    previous.shutdown().await;
                }
                *current = Some(handle);
            } else if let Some(handle) = current.as_ref() {
                remember_status(
                    state,
                    handle.update_catalog(catalog::runtime_catalog(&store)),
                )?;
            }
            remember_status(state, Ok(backup))
        }
        Err(error) => {
            if let Some(mut handle) = replacement {
                handle.shutdown().await;
            }
            Err(error)
        }
    }
}
