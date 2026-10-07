//! Compose the desktop shell and register the stable command surface.
mod commands;
mod credentials;
mod dashboard;
mod desktop;
mod evaluation;
mod inspection;
mod locations;
mod metadata;
mod runtime;
mod state;
mod threads;
mod updater;

use crate::core::AppPaths;
pub use credentials::run_credential_mode;
use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = match AppPaths::discover() {
        Ok(paths) => paths,
        Err(error) => {
            desktop::show_startup_error(&error);
            return;
        }
    };
    let mut context = tauri::generate_context!();
    let (artifact_previews, preview_frame_source) =
        tauri::async_runtime::block_on(crate::artifact_preview::ArtifactPreviewState::start())
            .unwrap_or_else(|_| {
                (
                    crate::artifact_preview::ArtifactPreviewState::default(),
                    "'none'".into(),
                )
            });
    let mut csp: std::collections::HashMap<String, tauri::utils::config::CspDirectiveSources> =
        context
            .config()
            .app
            .security
            .csp
            .clone()
            .unwrap_or_else(|| tauri::utils::config::Csp::Policy("default-src 'self'".into()))
            .into();
    csp.insert(
        "frame-src".into(),
        tauri::utils::config::CspDirectiveSources::Inline(preview_frame_source),
    );
    context.config_mut().app.security.csp = Some(tauri::utils::config::Csp::DirectiveMap(csp));
    context.config_mut().identifier = match paths.instance_identifier() {
        Ok(identifier) => identifier,
        Err(error) => {
            desktop::show_startup_error(&error);
            return;
        }
    };
    // Existing installs retain their WebView storage so the one-time theme-key
    // migration can read the user's preference after the application ID changes.
    let legacy_webview = match crate::core::legacy::legacy_webview_directory(&paths) {
        Ok(directory) => directory,
        Err(error) => {
            desktop::show_startup_error(&error);
            return;
        }
    };
    if let Some(directory) = legacy_webview {
        for window in &mut context.config_mut().app.windows {
            window.data_directory = Some(directory.clone());
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !args.iter().any(|arg| arg == "--background") {
                desktop::show_main(app);
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(artifact_previews)
        .setup(move |app| {
            // Activate pending storage changes only after single-instance
            // arbitration, before any background writer acquires these paths.
            let paths = locations::activate_pending(paths.clone());
            // Upgrade helpers before schedulers or the UI can acquire the same
            // nonblocking configuration lock. A failed migration stays visible
            // while diagnostics and explicit repairs remain available.
            let gateway_upgrade = crate::catalog::upgrade_legacy_connection(&paths);
            let direct_upgrade = crate::core::upgrade_legacy_direct_connection(&paths);
            let connection_upgrade = gateway_upgrade.and(direct_upgrade);
            let state = AppState::new(paths.clone());
            if let Ok(mut error) = state.connection_upgrade_error.lock() {
                *error = connection_upgrade.err();
            }
            app.manage(updater::UpdateState::new(paths.clone()));
            app.manage(state);
            app.manage(crate::evaluation::EvaluationState::new(paths.clone()));
            app.manage(crate::threads::ThreadState::new(paths));
            desktop::setup(app)?;
            metadata::start(app.handle().clone());
            updater::start(app.handle().clone());
            evaluation::start_scheduler(app.handle().clone());
            crate::threads::start_scheduler(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_dashboard,
            commands::profiles::save_profile,
            commands::profiles::delete_profile,
            commands::configuration::preview_profile,
            commands::configuration::apply_profile,
            commands::configuration::preview_restore,
            commands::configuration::restore_backup,
            commands::configuration::preview_repair,
            commands::configuration::apply_repair,
            commands::diagnostics::validate_profile,
            commands::diagnostics::run_diagnostics,
            commands::diagnostics::cancel_diagnostics,
            commands::open_codex,
            commands::profiles::sync_profile,
            commands::save_settings,
            commands::configuration::preview_gateway,
            commands::configuration::apply_gateway,
            updater::get_update_status,
            updater::check_for_updates,
            updater::download_update,
            updater::install_update,
            updater::set_update_preferences,
            evaluation::get_evaluation_dashboard,
            evaluation::get_evaluation_activity,
            evaluation::save_evaluation_plan,
            evaluation::start_evaluation,
            evaluation::cancel_evaluation,
            evaluation::get_evaluation_run,
            evaluation::delete_evaluation_runs,
            evaluation::export_evaluation_run,
            threads::get_thread_dashboard,
            threads::scan_threads,
            threads::get_thread_detail,
            threads::preview_thread_restore,
            threads::restore_thread,
            threads::get_thread_settings,
            threads::save_thread_settings,
            threads::reconcile_thread_index,
            threads::list_threads,
            threads::rebuild_thread_inventory,
            threads::open_thread,
            locations::get_location_preferences,
            locations::preview_location_preferences,
            locations::save_location_preferences,
            threads::preview_thread_deletion,
            threads::delete_threads,
            threads::list_thread_trash,
            threads::preview_thread_trash_restore,
            threads::restore_thread_trash,
            crate::artifact_preview::create_artifact_preview,
            crate::artifact_preview::release_artifact_preview
        ])
        .run(context)
        .unwrap_or_else(|error| desktop::show_startup_error(&format!("无法启动桌面程序：{error}")));
}
