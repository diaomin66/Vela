//! GET-only channel metadata refresh, shared by manual sync and the background scheduler.
use super::{runtime::refresh_gateway, state::AppState};
use crate::{
    core::{self, Profile},
    discovery,
};
use tauri::Manager;
use tokio::sync::watch;

pub(super) async fn synchronize(
    state: &AppState,
    id: &str,
    receiver: watch::Receiver<bool>,
) -> Result<Profile, String> {
    let (profile, key) = core::load_validation_profile(&state.paths, id)?;
    let result = discovery::discover(
        discovery::DiscoveryInput {
            base_url: profile.base_url.clone(),
            key: key.to_string(),
            balance_config: profile.balance_config.clone(),
        },
        receiver,
    )
    .await;
    let saved = core::record_discovery(&state.paths, id, &profile.revision, result)?;
    refresh_gateway(state).await?;
    Ok(saved)
}

pub(super) fn start(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let _ = refresh_gateway(&state).await;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let Ok(store) = core::load_store(&state.paths) else {
                continue;
            };
            if !store.settings.auto_refresh {
                continue;
            }
            for profile in store.profiles {
                let due = profile
                    .last_synced_at
                    .as_deref()
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                    .is_none_or(|time| {
                        chrono::Utc::now().signed_duration_since(time).num_minutes()
                            >= i64::from(store.settings.refresh_minutes)
                    });
                if due {
                    let (_sender, receiver) = watch::channel(false);
                    let _ = synchronize(&state, &profile.id, receiver).await;
                }
            }
        }
    });
}
