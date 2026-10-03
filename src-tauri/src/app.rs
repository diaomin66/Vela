//! Compose the desktop shell and register the stable command surface.
mod commands;
mod credentials;
mod dashboard;
mod desktop;
mod inspection;
mod metadata;
mod runtime;
mod state;
mod updater;
mod evaluation;

use crate::core::AppPaths;
pub use credentials::run_credential_mode;
use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = AppPaths::discover().expect("Unable to locate user configuration directories");
    let mut context = tauri::generate_context!();
    let (artifact_previews, preview_frame_source) = tauri::async_runtime::block_on(crate::artifact_preview::ArtifactPreviewState::start())
        .unwrap_or_else(|_| (crate::artifact_preview::ArtifactPreviewState::default(), "'none'".into()));
    let mut csp: std::collections::HashMap<String, tauri::utils::config::CspDirectiveSources> = context.config().app.security.csp.clone()
        .unwrap_or_else(|| tauri::utils::config::Csp::Policy("default-src 'self'".into())).into();
    csp.insert("frame-src".into(), tauri::utils::config::CspDirectiveSources::Inline(preview_frame_source));
    context.config_mut().app.security.csp = Some(tauri::utils::config::Csp::DirectiveMap(csp));
    context.config_mut().identifier = paths
        .instance_identifier()
        .expect("Unable to resolve application instance identity");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !args.iter().any(|arg| arg == "--background") {
                desktop::show_main(app);
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::UpdateState::new(paths.clone()))
        .manage(crate::evaluation::EvaluationState::new(paths.clone()))
        .manage(artifact_previews)
        .manage(AppState::new(paths))
        .setup(|app| {
            desktop::setup(app)?;
            metadata::start(app.handle().clone());
            updater::start(app.handle().clone());
            evaluation::start_scheduler(app.handle().clone());
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
            evaluation::export_evaluation_run,
            crate::artifact_preview::create_artifact_preview,
            crate::artifact_preview::release_artifact_preview
        ])
        .run(context)
        .expect("Unable to launch Vela");
}
