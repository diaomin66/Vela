//! Restart-applied locations with a stable identity and verified, non-destructive copies.
mod migration;
mod resolve;
mod types;
mod validation;

use crate::core::{self, AppPaths};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use types::*;
pub use types::{LocationPreferences, LocationPreview, LocationStatus, ResolvedLocations};

const PREFERENCES_FILE: &str = "locations.json";

pub(crate) fn credential_paths(mut paths: AppPaths) -> AppPaths {
    if std::env::var_os("CODEX_HOME").is_none_or(|value| value.is_empty()) {
        if let Some(home) = read(&paths)
            .ok()
            .and_then(|stored| stored.active.codex_home)
        {
            let home = std::path::PathBuf::from(home);
            if validation::directory(&home).is_ok() {
                paths.config = home.join("config.toml");
            }
        }
    }
    paths
}

pub(crate) fn discover(paths: AppPaths, user_home: &Path) -> Result<AppPaths, String> {
    let defaults = resolve::runtime_defaults(&paths, user_home)?;
    let preferences = read(&paths)?.active;
    let resolved = resolve::preferences(&paths, &preferences, &defaults)?;
    Ok(paths.with_locations(resolved))
}

/// Resolve each retained source independently. Historical sources must not inherit
/// the active home's environment or forget an explicitly selected database directory.
pub(crate) fn source_sqlite_home(
    paths: &AppPaths,
    home: &Path,
    remembered: Option<&str>,
) -> Result<std::path::PathBuf, String> {
    validation::directory(home)?;
    let sqlite = if validation::same(home, &paths.codex_home())? && paths.locations.is_some() {
        if let Some(error) = paths.location_error() {
            return Err(error.to_owned());
        }
        paths.sqlite_home()
    } else if let Some(remembered) = remembered {
        std::path::PathBuf::from(remembered)
    } else {
        resolve::config_sqlite_home(home, active(paths).defaults.user_home.as_deref())?
            .unwrap_or_else(|| home.to_path_buf())
    };
    validation::directory(&sqlite)?;
    Ok(sqlite)
}

fn read(paths: &AppPaths) -> Result<Persisted, String> {
    let path = paths.data.join(PREFERENCES_FILE);
    validation::guard(&path, true)?;
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Persisted::default())
        }
        Err(_) => return Err("无法读取存储位置设置。".into()),
    };
    if !metadata.is_file() || metadata.len() > 256 * 1024 {
        return Err("存储位置设置大小异常，原文件已保留。".into());
    }
    let value: Persisted =
        serde_json::from_slice(&fs::read(path).map_err(|_| "无法读取存储位置设置。")?)
            .map_err(|_| "存储位置设置无法解析，原文件已保留。")?;
    if value.version != 1 {
        return Err("存储位置设置来自不兼容版本，请更新应用。".into());
    }
    Ok(value)
}

fn write(paths: &AppPaths, value: &Persisted) -> Result<(), String> {
    validation::guard(&paths.data.join(PREFERENCES_FILE), true)?;
    core::atomic_write(
        &paths.data.join(PREFERENCES_FILE),
        &serde_json::to_vec_pretty(value).map_err(|_| "无法序列化存储位置设置。")?,
    )
}

fn active(paths: &AppPaths) -> ResolvedLocations {
    paths.locations.as_deref().cloned().unwrap_or_else(|| {
        ResolvedLocations::defaults(&paths.data, &paths.codex_home(), &paths.codex_home())
    })
}

pub(crate) fn status(paths: &AppPaths) -> Result<LocationStatus, String> {
    let stored = read(paths)?;
    let active = active(paths);
    let next_preferences = stored.pending.as_ref().unwrap_or(&stored.active);
    let next = resolve::preferences(paths, next_preferences, &active.defaults)?;
    Ok(LocationStatus {
        preferences: stored.active,
        pending_preferences: stored.pending.clone(),
        active: active.display(),
        next: next.display(),
        requires_restart: stored.pending.is_some(),
        anchor_directory: validation::display(&paths.data),
        overrides: resolve::overrides(&active.defaults),
        error: stored.error.or(next.error),
    })
}

fn preview_hash(
    stored: &Persisted,
    preferences: &LocationPreferences,
    resolved: &ResolvedLocations,
    changes: &[LocationChange],
) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(stored, preferences, resolved.display(), changes))
        .map_err(|_| "无法生成位置预览校验值。")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub(crate) fn preview(
    paths: &AppPaths,
    preferences: LocationPreferences,
) -> Result<LocationPreview, String> {
    let stored = read(paths)?;
    let preferences = resolve::normalize(preferences);
    let current = active(paths);
    let next = resolve::preferences(paths, &preferences, &current.defaults)?;
    let mut errors = Vec::new();
    if let Err(error) = validation::locations(paths, &next) {
        errors.push(error);
    }
    let changes = match migration::preview(&current, &next) {
        Ok(changes) => changes,
        Err(error) => {
            errors.push(error);
            Vec::new()
        }
    };
    let mut warnings = Vec::new();
    if let Some(error) = &next.error {
        warnings.push(error.clone());
    }
    if current.codex_home != next.codex_home || current.sqlite_home != next.sqlite_home {
        warnings.push("官方目录仅切换读取位置，原始会话和官方数据库留在原处；AhaX 会保留旧线程来源。外部客户端需要使用同一目录配置。".into());
    }
    if changes.iter().any(|change| change.migration == "copy") {
        warnings.push(
            "重启时复制并校验历史数据，成功后切换；旧目录仍保留，目标中的不同内容不会被覆盖。"
                .into(),
        );
    }
    if stored.pending.is_some() {
        warnings.push(
            "再次保存将替换待重启方案；保存当前生效的设置可取消迁移。已复制文件和原目录都会保留。"
                .into(),
        );
    }
    let requires_restart = current.display() != next.display() || preferences != stored.active;
    Ok(LocationPreview {
        expected_hash: preview_hash(&stored, &preferences, &next, &changes)?,
        preferences,
        resolved: next.display(),
        requires_restart,
        changes,
        warnings,
        can_save: errors.is_empty(),
        errors,
    })
}

#[cfg(test)]
pub(crate) fn save(
    paths: &AppPaths,
    preferences: LocationPreferences,
    expected_hash: &str,
) -> Result<LocationStatus, String> {
    save_with_checkpoint(paths, preferences, expected_hash, || Ok(()))
}

pub(crate) fn save_with_checkpoint(
    paths: &AppPaths,
    preferences: LocationPreferences,
    expected_hash: &str,
    checkpoint: impl FnOnce() -> Result<(), String>,
) -> Result<LocationStatus, String> {
    validation::guard(&paths.data, true)?;
    let _lock = paths.lock()?;
    let mut proposal = preview(paths, preferences)?;
    if expected_hash != proposal.expected_hash {
        return Err("位置设置或有效目录在预览后发生变化，请重新预览。".into());
    }
    if !proposal.can_save {
        return Err(proposal.errors.join(" "));
    }
    let mut stored = read(paths)?;
    if proposal
        .changes
        .iter()
        .any(|change| change.key == "codexHome")
    {
        checkpoint()?;
        let refreshed = preview(paths, proposal.preferences.clone())?;
        if refreshed.resolved != proposal.resolved || read(paths)? != stored {
            return Err("保护线程期间位置设置发生变化，请重新预览。".into());
        }
        if !refreshed.can_save {
            return Err(refreshed.errors.join(" "));
        }
        // Protection intentionally creates files after the user's preview. Recount
        // that data under the same application lock before saving the copy plan.
        proposal = refreshed;
    }
    // Abandon only the transaction record. Existing sources and copied targets remain
    // untouched, so a failed destination never traps the user in a migration loop.
    migration::abandon(paths)?;
    stored.pending = if proposal.requires_restart {
        Some(proposal.preferences)
    } else {
        None
    };
    stored.pending_resolved = stored.pending.as_ref().map(|_| proposal.resolved);
    stored.pending_sources = if stored.pending.is_some() {
        proposal
            .changes
            .iter()
            .filter(|change| change.migration == "copy" && Path::new(&change.current_path).is_dir())
            .map(|change| change.current_path.clone())
            .collect()
    } else {
        Vec::new()
    };
    stored.error = None;
    stored.revision = uuid::Uuid::new_v4().to_string();
    write(paths, &stored)?;
    status(paths)
}

/// Called only by the main desktop instance, before constructing background writers.
pub(crate) fn activate_pending(paths: AppPaths) -> Result<AppPaths, String> {
    validation::guard(&paths.data, true)?;
    let _lock = paths.lock()?;
    let mut stored = read(&paths)?;
    let current = active(&paths);
    let Some(preferences) = stored.pending.clone() else {
        // A crash may occur after preferences commit and before journal removal.
        if let Err(error) = migration::finish_committed(&paths, &current) {
            stored.error = Some(error);
            let _ = write(&paths, &stored);
        }
        return Ok(paths);
    };
    let result: Result<ResolvedLocations, String> = (|| {
        let next = resolve::preferences(&paths, &preferences, &current.defaults)?;
        validation::locations(&paths, &next)?;
        if stored
            .pending_resolved
            .as_ref()
            .is_some_and(|expected| *expected != next.display())
        {
            return Err("重启前有效目录发生变化，未迁移数据。请重新预览位置设置。".into());
        }
        migration::require_sources(&stored.pending_sources)?;
        migration::execute(&paths, &current, &next)?;
        let mut committed = stored.clone();
        committed.active = preferences;
        committed.pending = None;
        committed.pending_resolved = None;
        committed.pending_sources.clear();
        committed.error = None;
        committed.revision = uuid::Uuid::new_v4().to_string();
        write(&paths, &committed)?;
        // The preference commit is the activation boundary. A cleanup failure must
        // never roll the active location back after that durable write.
        if let Err(error) = migration::finish(&paths) {
            committed.error = Some(error);
            let _ = write(&paths, &committed);
        }
        Ok(next)
    })();
    match result {
        Ok(next) => Ok(paths.with_locations(next)),
        Err(error) => {
            stored.error = Some(error.clone());
            let _ = write(&paths, &stored);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests;
