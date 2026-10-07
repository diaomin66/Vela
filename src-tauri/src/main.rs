#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if ahax_lib::run_credential_mode() {
        return;
    }
    ahax_lib::run();
}
