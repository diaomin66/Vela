//! Read-only view construction; environment detection never runs on the UI thread.
use super::{desktop::desktop_info, state::AppState};
use crate::{
    catalog,
    core::{self, AppPaths, Backup, Profile, Settings},
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Environment {
    platform: String,
    codex_installed: bool,
    codex_version: Option<String>,
    config_path: String,
    config_exists: bool,
    config_valid: bool,
    app_version: String,
    desktop_mode: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Dashboard {
    environment: Environment,
    profiles: Vec<Profile>,
    active_profile_id: Option<String>,
    active_profile_matches: bool,
    backups: Vec<Backup>,
    settings: Settings,
    gateway: GatewayView,
    catalog: Vec<catalog::ModelCatalogEntry>,
    gateway_applied: bool,
    gateway_configured: bool,
    default_route_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewayView {
    running: bool,
    port: u16,
    base_url: String,
    error: Option<String>,
}
fn dashboard(paths: &AppPaths, gateway: GatewayView) -> Result<Dashboard, String> {
    let store = core::load_store(paths)?;
    let current = core::read_config(paths);
    let (exists, valid, active) = match &current {
        Ok(bytes) => match core::config_text(bytes) {
            Ok(text) => (
                bytes.is_some(),
                text.parse::<toml_edit::DocumentMut>().is_ok(),
                core::active_profile_id(text, &store.profiles),
            ),
            Err(_) => (bytes.is_some(), false, None),
        },
        Err(_) => (paths.config.exists(), false, None),
    };
    let desktop = desktop_info();
    let active_matches = active
        .as_ref()
        .and_then(|id| store.profiles.iter().find(|p| &p.id == id))
        .and_then(|profile| {
            current
                .as_ref()
                .ok()
                .and_then(|bytes| core::config_text(bytes).ok())
                .map(|text| core::profile_matches_configuration(text, profile, &paths.helper))
        })
        .unwrap_or(false);
    let gateway_applied = current
        .as_ref()
        .ok()
        .and_then(|b| core::config_text(b).ok())
        .is_some_and(|text| catalog::applied(paths, &store, text));
    let gateway_configured = current
        .as_ref()
        .ok()
        .and_then(|bytes| core::config_text(bytes).ok())
        .is_some_and(catalog::is_gateway_config);
    let catalog = catalog::entries(&store.profiles);
    Ok(Dashboard {
        environment: Environment {
            platform: std::env::consts::OS.into(),
            codex_installed: desktop.installed(),
            codex_version: desktop.version,
            config_path: paths.config.to_string_lossy().into(),
            config_exists: exists,
            config_valid: valid,
            app_version: env!("CARGO_PKG_VERSION").into(),
            desktop_mode: true,
        },
        active_profile_id: active,
        active_profile_matches: active_matches,
        settings: store.settings.clone(),
        gateway,
        catalog,
        gateway_applied,
        gateway_configured,
        default_route_id: store
            .default_route_id
            .as_deref()
            .map(catalog::canonical_route_id),
        profiles: store.profiles,
        backups: store
            .backups
            .into_iter()
            .filter(|b| b.config_path == paths.config.to_string_lossy())
            .collect(),
    })
}

pub(super) async fn snapshot(state: &AppState) -> Result<Dashboard, String> {
    let paths = state.paths.clone();
    let port = core::load_store(&paths)?.settings.gateway_port;
    let handle = state.gateway.lock().await;
    let status = handle.as_ref().map(|h| h.status());
    let gateway = GatewayView {
        running: status.as_ref().is_some_and(|s| s.running),
        port: status.as_ref().map(|s| s.port).unwrap_or(port),
        base_url: format!(
            "http://127.0.0.1:{}/v1",
            status.as_ref().map(|s| s.port).unwrap_or(port)
        ),
        error: state
            .gateway_error
            .lock()
            .ok()
            .and_then(|e| e.clone())
            .or_else(|| {
                state
                    .connection_upgrade_error
                    .lock()
                    .ok()
                    .and_then(|e| e.clone())
            }),
    };
    drop(handle);
    tauri::async_runtime::spawn_blocking(move || dashboard(&paths, gateway))
        .await
        .map_err(|_| "无法完成环境检测。".to_string())?
}
