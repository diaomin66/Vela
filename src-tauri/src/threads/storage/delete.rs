//! Reversible deletion coordinates protection and the official local service.
//! The journal is authoritative; the disposable SQLite inventory is not.
mod actions;
mod journal;
mod plan;

pub(in crate::threads) use actions::{delete, preview_trash_restore, restore_trash};
pub(in crate::threads) use journal::{audit, list_trash, tombstones};
pub(in crate::threads) use plan::preview_delete;

#[cfg(test)]
mod tests;
