use crate::core::AppPaths;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub(super) fn directory(paths: &AppPaths) -> PathBuf {
    paths.data.join("threads")
}

pub(super) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn safe_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err("线程来源标识无效。".into());
    }
    Ok(())
}

pub(super) fn safe_hash(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("线程快照校验标识无效。".into());
    }
    Ok(())
}

pub(super) fn relative(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value.replace('\\', "/"));
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Normal(part)) if part == "sessions" || part == "archived_sessions")
    {
        return Err("线程文件不属于受支持的会话目录。".into());
    }
    if !components.all(|part| matches!(part, Component::Normal(_))) {
        return Err("线程文件路径包含不安全的目录部分。".into());
    }
    let name = path
        .file_name()
        .and_then(|part| part.to_str())
        .ok_or("线程文件名无效。")?;
    if !name.starts_with("rollout-") || !(name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
    {
        return Err("此文件不是受支持的线程记录。".into());
    }
    if path.components().count() < 2 {
        return Err("线程记录路径无效。".into());
    }
    Ok(path)
}

pub(super) fn source_root(path: &Path, relative_path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("线程来源路径必须为绝对路径。".into());
    }
    let mut root = path.to_path_buf();
    for _ in relative_path.components() {
        if !root.pop() {
            return Err("线程来源路径无效。".into());
        }
    }
    if normalized(&root.join(relative_path))? != normalized(path)? {
        return Err("线程来源与记录路径不匹配。".into());
    }
    Ok(root)
}

pub(super) fn normalized(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err("线程目录必须为绝对路径。".into());
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("线程目录不能包含父目录跳转。".into());
    }
    let absolute = std::path::absolute(path).map_err(|_| "无法解析线程目录。")?;
    #[cfg(windows)]
    {
        let value = absolute.to_string_lossy().replace('/', "\\").to_lowercase();
        let value = if let Some(rest) = value.strip_prefix("\\\\?\\unc\\") {
            format!("\\\\{rest}")
        } else {
            value.strip_prefix("\\\\?\\").unwrap_or(&value).to_owned()
        };
        Ok(value.trim_end_matches('\\').to_owned())
    }
    #[cfg(not(windows))]
    {
        Ok(absolute.to_string_lossy().trim_end_matches('/').to_owned())
    }
}

pub(super) fn reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub(super) fn guard_path(path: &Path, allow_missing: bool) -> Result<(), String> {
    normalized(path)?;
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if reparse(&metadata) => {
                return Err("线程路径包含符号链接或目录联接，未读取或修改该路径。".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_missing => {}
            Err(_) => return Err("无法验证线程目录，请检查位置和访问权限。".into()),
        }
    }
    Ok(())
}

pub(super) fn ensure_directory(path: &Path) -> Result<(), String> {
    guard_path(path, true)?;
    fs::create_dir_all(path).map_err(|_| "无法创建线程保护目录。")?;
    guard_path(path, false)
}
