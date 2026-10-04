use chrono::{DateTime, Utc};
use std::{
    fs::{self, File, Metadata},
    io::{self, Read},
    path::{Path, PathBuf},
};

pub(super) const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 200_000;

pub(super) struct Discovery {
    pub paths: Vec<(PathBuf, bool)>,
    pub error: Option<String>,
}

pub(super) enum ReadFailure {
    TooLarge(u64),
    Changed,
    Unreadable,
}

pub(super) struct StableRead {
    pub bytes: Vec<u8>,
    pub modified_at: Option<String>,
}

pub(super) fn root_available(root: &Path) -> bool {
    if !root.is_absolute() {
        return false;
    }
    let mut current = PathBuf::new();
    for component in root.components() {
        current.push(component);
        if matches!(component, std::path::Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if !is_link(&metadata) => {}
            _ => return false,
        }
    }
    root.is_dir()
}

pub(super) fn root_missing_safe(root: &Path) -> bool {
    if !root.is_absolute() {
        return false;
    }
    let mut current = PathBuf::new();
    let mut missing = false;
    for component in root.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return false;
        }
        current.push(component);
        if matches!(component, std::path::Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if !is_link(&metadata) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing = true;
            }
            _ => return false,
        }
    }
    missing
}

pub(super) fn discover(root: &Path, include_archived: bool) -> Discovery {
    let mut stack = vec![(root.join("sessions"), false, 0usize)];
    if include_archived {
        stack.push((root.join("archived_sessions"), true, 0));
    }
    let mut paths = Vec::new();
    let mut count = 0;
    let mut error = None;
    while let Some((directory, archived, depth)) = stack.pop() {
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.is_dir() && !is_link(&metadata) => {}
            Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => continue,
            _ => {
                error = Some("部分会话目录不可读取或使用了链接，已保留之前的保护记录。".into());
                continue;
            }
        }
        let Ok(entries) = fs::read_dir(directory) else {
            error = Some("部分会话目录无法读取，已保留之前的保护记录。".into());
            continue;
        };
        for entry in entries {
            count += 1;
            if count > MAX_ENTRIES {
                return Discovery {
                    paths,
                    error: Some("会话文件数量超过本轮扫描上限，未扫描记录已保留。".into()),
                };
            }
            let Ok(entry) = entry else {
                error = Some("部分会话条目无法读取。".into());
                continue;
            };
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                error = Some("部分会话条目无法读取，已保留之前的保护记录。".into());
                continue;
            };
            if is_link(&metadata) {
                error =
                    Some("部分会话条目使用了链接，本轮未确认其内容，已保留之前的保护记录。".into());
                continue;
            }
            if metadata.is_dir() {
                if depth < 12 {
                    stack.push((entry.path(), archived, depth + 1));
                } else {
                    error = Some("部分会话目录超过本轮扫描深度，已保留之前的保护记录。".into());
                }
            } else if metadata.is_file() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("rollout-")
                    && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
                {
                    paths.push((entry.path(), archived));
                }
            }
        }
    }
    paths.sort_by(|left, right| left.0.cmp(&right.0));
    Discovery { paths, error }
}

pub(super) fn stable_read(root: &Path, path: &Path) -> Result<StableRead, ReadFailure> {
    validate_path(root, path)?;
    let before = fs::symlink_metadata(path).map_err(|_| ReadFailure::Unreadable)?;
    if before.len() > MAX_BYTES {
        return Err(ReadFailure::TooLarge(before.len()));
    }
    let file = File::open(path).map_err(|_| ReadFailure::Unreadable)?;
    let opened = file.metadata().map_err(|_| ReadFailure::Unreadable)?;
    if !same_version(&before, &opened) {
        return Err(ReadFailure::Changed);
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    let mut reader = file.take(MAX_BYTES + 1);
    reader
        .read_to_end(&mut bytes)
        .map_err(|_| ReadFailure::Unreadable)?;
    let opened_after = reader
        .get_ref()
        .metadata()
        .map_err(|_| ReadFailure::Unreadable)?;
    let after = fs::metadata(path).map_err(|_| ReadFailure::Changed)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(ReadFailure::TooLarge(bytes.len() as u64));
    }
    if bytes.len() as u64 != before.len()
        || !same_version(&before, &opened_after)
        || !same_version(&before, &after)
    {
        return Err(ReadFailure::Changed);
    }
    Ok(StableRead {
        bytes,
        modified_at: before
            .modified()
            .ok()
            .map(DateTime::<Utc>::from)
            .map(|time| time.to_rfc3339()),
    })
}

pub(super) fn validate_path(root: &Path, path: &Path) -> Result<(), ReadFailure> {
    if !root_available(root) {
        return Err(ReadFailure::Unreadable);
    }
    let canonical_root = fs::canonicalize(root).map_err(|_| ReadFailure::Unreadable)?;
    let canonical_path = fs::canonicalize(path).map_err(|_| ReadFailure::Unreadable)?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(ReadFailure::Unreadable);
    }
    let before = fs::symlink_metadata(path).map_err(|_| ReadFailure::Unreadable)?;
    if !before.is_file() || is_link(&before) {
        return Err(ReadFailure::Unreadable);
    }
    let mut current = root.to_path_buf();
    for component in path
        .strip_prefix(root)
        .map_err(|_| ReadFailure::Unreadable)?
        .components()
    {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(ReadFailure::Unreadable);
        }
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(|_| ReadFailure::Unreadable)?;
        if is_link(&metadata) {
            return Err(ReadFailure::Unreadable);
        }
    }
    Ok(())
}

pub(super) fn same_version(left: &Metadata, right: &Metadata) -> bool {
    left.len() == right.len() && left.modified().ok() == right.modified().ok()
}

fn is_link(metadata: &Metadata) -> bool {
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

pub(super) fn modified_at(path: &Path) -> Option<String> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()
        .map(DateTime::<Utc>::from)
        .map(|time| time.to_rfc3339())
}

pub(super) fn normalized_path(path: &Path) -> String {
    let path = display_path(path);
    #[cfg(windows)]
    {
        path.replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    }
    #[cfg(not(windows))]
    {
        path.trim_end_matches('/').to_owned()
    }
}

pub(super) fn display_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    }
}
