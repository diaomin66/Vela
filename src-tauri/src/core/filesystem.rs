//! Bounded reads and durable atomic replacement. No business-level mutation policy.
use super::paths::AppPaths;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use uuid::Uuid;
use zeroize::Zeroizing;

const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;

pub fn read_config(paths: &AppPaths) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    match fs::metadata(&paths.config) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("无法读取配置文件信息，请检查访问权限。".into()),
        Ok(meta) if meta.len() > MAX_CONFIG_BYTES => {
            return Err("配置文件超过 4 MB，暂不自动编辑。".into())
        }
        Ok(meta) if !meta.is_file() => return Err("配置路径不是普通文件。".into()),
        Ok(_) => {}
    }
    fs::read(&paths.config)
        .map(|v| Some(Zeroizing::new(v)))
        .map_err(|_| "无法读取配置文件，请检查访问权限。".into())
}
pub fn config_text(bytes: &Option<Zeroizing<Vec<u8>>>) -> Result<&str, String> {
    std::str::from_utf8(bytes.as_deref().map(|v| v.as_slice()).unwrap_or_default())
        .map_err(|_| "配置文件不是有效的 UTF-8 文本，请从备份恢复。".into())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("文件路径无效。")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建目标目录，请检查访问权限。")?;
    let temporary = parent.join(format!(".vela-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "无法创建临时文件。")?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "文件写入失败，原文件保持不变。")?;
        drop(file);
        atomic_replace(&temporary, path)
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::*;
    let from: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let to: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let succeeded = unsafe {
        if target.exists() {
            ReplaceFileW(
                to.as_ptr(),
                from.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        } else {
            MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH)
        }
    };
    if succeeded == 0 {
        Err("无法原子替换文件，可能被其他程序占用。原文件未被主动删除。".into())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), String> {
    fs::rename(source, target).map_err(|_| "无法原子替换文件。".into())
}
