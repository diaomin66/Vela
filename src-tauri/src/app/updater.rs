//! Signed application updates. Background work never interrupts the gateway;
//! only the explicit install command launches an installer and exits Vela.
mod state;

use self::state::{
    read_preferences, write_preferences, UpdateMetadata, UpdatePhase, UpdateSession, UpdateStatus,
};
use crate::core::AppPaths;
use chrono::Utc;
use std::{
    sync::{Mutex, MutexGuard},
    time::Duration,
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

const STARTUP_DELAY: Duration = Duration::from_secs(15);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub(super) struct UpdateState {
    paths: AppPaths,
    session: Mutex<RuntimeSession>,
}

struct RuntimeSession {
    model: UpdateSession,
    update: Option<Update>,
}

impl UpdateState {
    pub(super) fn new(paths: AppPaths) -> Self {
        let model = UpdateSession::new(read_preferences(&paths));
        Self {
            paths,
            session: Mutex::new(RuntimeSession {
                model,
                update: None,
            }),
        }
    }
    fn lock(&self) -> Result<MutexGuard<'_, RuntimeSession>, String> {
        self.session
            .lock()
            .map_err(|_| "更新状态暂时不可用，请重新打开 Vela。".into())
    }
}

pub(super) fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            // Network failures live only in the updater panel; never interrupt work with alerts.
            let _ = start_check(&app);
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

fn start_check(app: &AppHandle) -> Result<UpdateStatus, String> {
    let state = app.state::<UpdateState>();
    let mut session = state.lock()?;
    if session.model.begin_check() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            check(app).await;
        });
    }
    Ok(session.model.status.clone())
}

async fn check(app: AppHandle) {
    // Endpoints and public key come exclusively from the packaged plugin config.
    // No frontend command can supply an arbitrary download URL or trust key.
    let result = match app.updater_builder().timeout(CHECK_TIMEOUT).build() {
        Ok(updater) => updater.check().await,
        Err(error) => Err(error),
    };
    let download = {
        let state = app.state::<UpdateState>();
        let Ok(mut session) = state.lock() else {
            return;
        };
        match result {
            Ok(update) => {
                let metadata = update.as_ref().map(|candidate| UpdateMetadata {
                    version: candidate.version.clone(),
                    notes: candidate.body.clone(),
                    identity: format!(
                        "{}\n{}\n{}",
                        candidate.version, candidate.signature, candidate.download_url
                    ),
                });
                session.update = update.map(|mut update| {
                    update.timeout = Some(DOWNLOAD_TIMEOUT);
                    update
                });
                session.model.checked(metadata)
            }
            Err(_) => {
                session.model.status.checked_at = Some(Utc::now().to_rfc3339());
                session
                    .model
                    .fail("暂时无法检查更新，请检查网络后重试。".into());
                false
            }
        }
    };
    if download {
        let _ = start_download(&app, true);
    }
}

fn start_download(app: &AppHandle, automatic: bool) -> Result<UpdateStatus, String> {
    let state = app.state::<UpdateState>();
    let mut session = state.lock()?;
    if automatic && !session.model.status.auto_download {
        return Ok(session.model.status.clone());
    }
    if session.model.begin_download() {
        let update = session
            .update
            .clone()
            .ok_or("更新信息不完整，请重新检查更新。")?;
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            download(app, update).await;
        });
    }
    Ok(session.model.status.clone())
}

async fn download(app: AppHandle, update: Update) {
    let result = update
        .download(
            |count, total| {
                let state = app.state::<UpdateState>();
                if let Ok(mut session) = state.lock() {
                    session.model.status.downloaded_bytes = session
                        .model
                        .status
                        .downloaded_bytes
                        .saturating_add(count as u64);
                    session.model.status.total_bytes = total;
                };
            },
            || {},
        )
        .await;
    let state = app.state::<UpdateState>();
    if let Ok(mut session) = state.lock() {
        match result {
            Ok(bytes) => session.model.downloaded(bytes),
            Err(error) => session.model.fail(download_error(&error)),
        }
    };
}

fn download_error(error: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error;
    match error {
        Error::Minisign(_)
        | Error::Base64(_)
        | Error::SignatureUtf8(_)
        | Error::SignedVersionMismatch { .. }
        | Error::MissingSignedVersion => "更新包签名验证失败，已阻止安装。请稍后重试。".into(),
        _ => "更新下载未完成，请检查网络后重试。".into(),
    }
}

#[tauri::command]
pub(super) fn get_update_status(state: State<'_, UpdateState>) -> Result<UpdateStatus, String> {
    Ok(state.lock()?.model.status.clone())
}

#[tauri::command]
pub(super) fn check_for_updates(app: AppHandle) -> Result<UpdateStatus, String> {
    start_check(&app)
}

#[tauri::command]
pub(super) fn download_update(app: AppHandle) -> Result<UpdateStatus, String> {
    start_download(&app, false)
}

#[tauri::command]
pub(super) fn set_update_preferences(
    app: AppHandle,
    auto_download: bool,
) -> Result<UpdateStatus, String> {
    let state = app.state::<UpdateState>();
    let should_download = {
        let mut session = state.lock()?;
        write_preferences(&state.paths, auto_download)?;
        session.model.status.auto_download = auto_download;
        auto_download && session.model.status.phase == UpdatePhase::Available
    };
    if should_download {
        return start_download(&app, true);
    }
    let status = state.lock()?.model.status.clone();
    Ok(status)
}

#[tauri::command]
pub(super) fn install_update(app: AppHandle) -> Result<UpdateStatus, String> {
    let state = app.state::<UpdateState>();
    let mut session = state.lock()?;
    if session.model.busy() {
        return Ok(session.model.status.clone());
    }
    let update = session.update.clone().ok_or("请先检查并下载更新。")?;
    let bytes = session
        .model
        .verified_bytes
        .clone()
        .ok_or("更新包尚未完成签名验证，请等待下载完成。")?;
    session.model.status.phase = UpdatePhase::Installing;
    session.model.status.error = None;
    let status = session.model.status.clone();
    drop(session);
    tauri::async_runtime::spawn_blocking(move || {
        // On Windows the official plugin launches the NSIS updater, restarts the
        // installed application after completion, and exits this process only
        // after ShellExecute succeeds. Keep verified bytes if launch fails.
        if update.install(bytes.as_slice()).is_err() {
            let state = app.state::<UpdateState>();
            if let Ok(mut session) = state.lock() {
                session
                    .model
                    .fail("无法启动安装程序，请关闭其他 Vela 窗口后重试。".into());
            };
        }
    });
    Ok(status)
}
