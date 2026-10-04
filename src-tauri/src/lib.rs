pub mod catalog;
// Headless tests omit the desktop commands that consume part of this crate-only facade.
#[cfg_attr(test, allow(dead_code, unused_imports))]
pub mod core;
pub mod diagnostics;
pub mod discovery;
pub mod gateway;
pub mod evaluation;
pub mod artifact_preview;
pub mod threads;
pub mod locations;
mod reasoning;
mod security;

// Domain and application-state tests have no GUI runtime dependency. This avoids requiring
// Common Controls activation manifests in Cargo's Windows test harness.
#[cfg(test)]
#[allow(dead_code)]
mod app {
    mod state;
    mod updater {
        mod state;
    }
}
#[cfg(not(test))]
mod app;
#[cfg(not(test))]
pub use app::{run, run_credential_mode};
