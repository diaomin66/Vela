#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if vela_lib::run_credential_mode() {
        return;
    }
    vela_lib::run();
}
