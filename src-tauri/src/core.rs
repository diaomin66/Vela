//! Application domain and persistence facade.
//!
//! Business operations own locking, revision checks, and backup policy. Storage
//! implementation modules stay private and the desktop shell uses a crate-only
//! operation facade. Public domain types keep the catalog schema explicit.
mod backups;
mod changes;
mod configuration;
mod domain;
mod filesystem;
pub(crate) mod legacy;
mod paths;
mod profiles;
mod repair;
mod store;
mod upgrade;

pub(crate) use backups::{commit_config, preview_restore, restore_backup};
pub(crate) use configuration::{
    active_profile_id, apply_profile, preview_profile, profile_matches_configuration, provider_id,
};
pub(crate) use domain::validate_id;
pub use domain::{
    Backup, Change, ChangePreview, ChannelModel, NativeReasoning, Profile, ProfileInput, Settings, Store,
};
pub(crate) use filesystem::{atomic_write, config_text, read_config};
pub use paths::AppPaths;
pub(crate) use profiles::{
    delete_profile, load_validation_profile, mark_validated, record_discovery, save_profile,
};
pub(crate) use repair::{apply_repair, preview_repair};
pub(crate) use store::save_store;
pub(crate) use store::{load_store, save_settings};
pub(crate) use upgrade::upgrade_legacy_direct_connection;

#[cfg(test)]
mod tests;
