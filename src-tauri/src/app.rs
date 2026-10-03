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

use crate::core::AppPaths;
pub use credentials::run_credential_mode;
use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = AppPaths::discover().expect("Unable to locate user configuration directories");
    let mut context = tauri::generate_context!();
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
        .manage(AppState::new(paths))
        .setup(|app| {
            desktop::setup(app)?;
            metadata::start(app.handle().clone());
            updater::start(app.handle().clone());
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
            updater::set_update_preferences
        ])
        .run(context)
        .expect("Unable to launch Vela");
}
