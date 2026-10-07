use super::types::ResolvedLocations;
use crate::core::AppPaths;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub(super) fn absolute(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("目录必须是绝对路径，且不能包含父目录跳转。".into());
    }
    if path
        .as_os_str()
        .to_string_lossy()
        .chars()
        .any(char::is_control)
    {
        return Err("目录包含不可用控制字符。".into());
    }
    Ok(())
}

pub(super) fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(crate) fn guard(path: &Path, allow_missing: bool) -> Result<(), String> {
    absolute(path)?;
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if reparse(&metadata) => {
                return Err("目录包含符号链接或目录联接，未访问其内容。".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_missing => {}
            Err(_) => return Err("目录不可读取，请检查位置和访问权限。".into()),
        }
    }
    Ok(())
}

pub(super) fn directory(path: &Path) -> Result<(), String> {
    guard(path, true)?;
    if fs::metadata(path).is_ok_and(|metadata| !metadata.is_dir()) {
        return Err("目录位置已被普通文件占用。".into());
    }
    if path.parent().is_none()
        || !path
            .components()
            .any(|part| matches!(part, Component::Normal(_)))
    {
        return Err("请使用专用子目录，不能直接使用磁盘根目录。".into());
    }
    Ok(())
}

pub(super) fn normalized(path: &Path) -> Result<PathBuf, String> {
    absolute(path)?;
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let display = display(&resolved);
    #[cfg(windows)]
    {
        Ok(PathBuf::from(display.replace('/', "\\").to_lowercase()))
    }
    #[cfg(not(windows))]
    {
        Ok(PathBuf::from(display))
    }
}

pub(super) fn same(left: &Path, right: &Path) -> Result<bool, String> {
    Ok(normalized(left)? == normalized(right)?)
}
pub(super) fn overlaps(left: &Path, right: &Path) -> Result<bool, String> {
    let left = normalized(left)?;
    let right = normalized(right)?;
    Ok(left.starts_with(&right) || right.starts_with(&left))
}

pub(super) fn locations(paths: &AppPaths, value: &ResolvedLocations) -> Result<(), String> {
    let stores = [
        &value.backups_directory,
        &value.evaluations_directory,
        &value.exports_directory,
        &value.thread_protection_directory,
        &value.thread_index_directory,
    ];
    directory(&value.codex_home)?;
    directory(&value.sqlite_home)?;
    for path in stores {
        directory(path)?;
        if overlaps(path, &value.codex_home)? || overlaps(path, &value.sqlite_home)? {
            return Err("ahaX 保存目录不能与官方配置、索引或会话目录重叠。".into());
        }
        if normalized(&paths.data)?.starts_with(normalized(path)?) {
            return Err("保存目录不能覆盖应用身份与设置目录。".into());
        }
        if normalized(path)?.starts_with(normalized(&paths.data)?) {
            let relative = normalized(path)?
                .strip_prefix(normalized(&paths.data)?)
                .map_err(|_| "应用数据目录无法解析。")?
                .to_path_buf();
            let permitted = relative.components().next().is_some_and(|component| {
                matches!(
                    component.as_os_str().to_str(),
                    Some("backups" | "evaluations" | "threads")
                )
            });
            if !permitted {
                return Err("应用身份目录内仅允许使用既有 backups、evaluations、threads 子目录；其他位置请选在应用身份目录外。".into());
            }
        }
    }
    for (i, left) in stores.iter().enumerate() {
        for (j, right) in stores.iter().enumerate().skip(i + 1) {
            let allowed = (i == 1 && j == 2 && same(right, &left.join("exports"))?)
                || (i == 3 && j == 4 && same(left, right)?);
            if !allowed && overlaps(left, right)? {
                return Err("独立保存目录不能相同或相互嵌套；请选择互不重叠的子目录。".into());
            }
        }
    }
    Ok(())
}

pub(super) fn display(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(&value).to_owned()
    }
}
