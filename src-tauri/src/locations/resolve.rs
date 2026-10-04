use super::{types::*, validation};
use crate::core::AppPaths;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn runtime_defaults(paths: &AppPaths, user_home: &Path) -> Result<Defaults, String> {
    let environment_home = environment_path("CODEX_HOME")?;
    let environment_sqlite = environment_path("CODEX_SQLITE_HOME")?;
    Ok(Defaults {
        codex_home: paths
            .config
            .parent()
            .ok_or("无法确定默认配置目录。")?
            .to_path_buf(),
        user_home: Some(user_home.to_path_buf()),
        environment_home,
        environment_sqlite,
    })
}

fn environment_path(name: &str) -> Result<Option<PathBuf>, String> {
    let Some(value) = std::env::var_os(name).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    validation::absolute(&path)?;
    Ok(Some(path))
}

pub(super) fn normalize(mut preferences: LocationPreferences) -> LocationPreferences {
    for value in [
        &mut preferences.codex_home,
        &mut preferences.sqlite_home,
        &mut preferences.backups_directory,
        &mut preferences.evaluations_directory,
        &mut preferences.exports_directory,
        &mut preferences.thread_protection_directory,
        &mut preferences.thread_index_directory,
    ] {
        *value = value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
    }
    preferences
}

fn selected(value: &Option<String>, default: &Path) -> Result<PathBuf, String> {
    let path = value
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| default.to_path_buf());
    validation::absolute(&path)?;
    std::path::absolute(path).map_err(|_| "无法解析目录位置。".into())
}

pub(super) fn preferences(
    paths: &AppPaths,
    input: &LocationPreferences,
    defaults: &Defaults,
) -> Result<ResolvedLocations, String> {
    let home = defaults
        .environment_home
        .clone()
        .map(Ok)
        .unwrap_or_else(|| selected(&input.codex_home, &defaults.codex_home))?;
    // Explicit environment values isolate test/portable launches. The app-server adapter
    // also sends this resolved value as a configuration override, preserving scope agreement.
    let (sqlite, error) = if let Some(environment) = &defaults.environment_sqlite {
        (environment.clone(), None)
    } else if input.sqlite_home.is_some() {
        (selected(&input.sqlite_home, &home)?, None)
    } else {
        match config_sqlite_home(&home, defaults.user_home.as_deref()) {
            Ok(path) => (path.unwrap_or_else(|| home.clone()), None),
            Err(error) => (home.clone(), Some(error)),
        }
    };
    let evaluations = selected(
        &input.evaluations_directory,
        &paths.data.join("evaluations"),
    )?;
    let protection = selected(
        &input.thread_protection_directory,
        &paths.data.join("threads"),
    )?;
    Ok(ResolvedLocations {
        codex_home: home,
        sqlite_home: sqlite,
        backups_directory: selected(&input.backups_directory, &paths.data.join("backups"))?,
        exports_directory: selected(&input.exports_directory, &evaluations.join("exports"))?,
        evaluations_directory: evaluations,
        thread_index_directory: selected(&input.thread_index_directory, &protection)?,
        thread_protection_directory: protection,
        defaults: defaults.clone(),
        error,
    })
}

pub(super) fn config_sqlite_home(
    home: &Path,
    user_home: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    let path = home.join("config.toml");
    validation::guard(&path, true)?;
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("无法读取官方 sqlite_home 配置。".into()),
    };
    if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 {
        return Err("官方配置超过读取范围，无法确认 SQLite 目录。".into());
    }
    let contents =
        fs::read_to_string(path).map_err(|_| "官方配置不是有效文本，无法确认 SQLite 目录。")?;
    // Existing diagnostics remains available for a malformed configuration. No external
    // metadata writes run until the app-server scope is verified separately.
    let document = contents
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| "官方配置无法解析，SQLite 目录尚未确认；请先在诊断中修复配置。")?;
    let Some(item) = document.get("sqlite_home") else {
        return Ok(None);
    };
    let value = item
        .as_str()
        .ok_or("官方 sqlite_home 必须是目录字符串。")?
        .trim();
    if value.is_empty() {
        return Ok(None);
    }
    let raw = if value == "~" {
        user_home
            .ok_or("此隔离环境不能展开用户目录。")?
            .to_path_buf()
    } else if value.starts_with("~/") || value.starts_with("~\\") {
        user_home
            .ok_or("此隔离环境不能展开用户目录。")?
            .join(&value[2..])
    } else {
        PathBuf::from(value)
    };
    let resolved = if raw.is_absolute() {
        raw
    } else {
        home.join(raw)
    };
    let resolved = std::path::absolute(resolved).map_err(|_| "无法解析官方 sqlite_home。")?;
    validation::absolute(&resolved)?;
    Ok(Some(resolved))
}

pub(super) fn overrides(defaults: &Defaults) -> Vec<LocationOverride> {
    let mut values = Vec::new();
    for (key, environment, path) in [
        ("codexHome", "CODEX_HOME", &defaults.environment_home),
        (
            "sqliteHome",
            "CODEX_SQLITE_HOME",
            &defaults.environment_sqlite,
        ),
    ] {
        if let Some(path) = path {
            values.push(LocationOverride {
                key: key.into(),
                environment: environment.into(),
                value: validation::display(path),
            });
        }
    }
    values
}
