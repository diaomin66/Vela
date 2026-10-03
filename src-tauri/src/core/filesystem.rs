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
    let mut preserve_temporary = false;
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
        match atomic_replace(&temporary, path) {
            Ok(()) => Ok(()),
            Err(error) => {
                preserve_temporary = preserve_replacement(&error);
                Err(replacement_error(&error))
            }
        }
    })();
    // Write failures before replacement can safely clean their own temporary
    // file. Replacement failures own their cleanup policy: some Windows error
    // codes describe partially completed operations and require preserving it.
    if !preserve_temporary && temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn preserve_replacement(error: &std::io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(1176 | 1177))
}

fn replacement_error(error: &std::io::Error) -> String {
    #[cfg(windows)]
    let detail = format!("（Win32 错误码 {}）", error.raw_os_error().unwrap_or(0));
    #[cfg(not(windows))]
    let detail = String::new();
    if preserve_replacement(error) {
        format!("文件替换未完整完成{detail}。暂存文件已保留在同目录，请检查后再继续。")
    } else {
        format!("无法原子替换文件{detail}，请检查文件是否被占用或是否有写入权限。")
    }
}

#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> std::io::Result<()> {
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
    let existing = target.exists();
    retry_sharing_violation(
        || {
            let succeeded = unsafe {
                if existing {
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
                // Capture immediately, before filesystem checks can replace the code.
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        },
        std::thread::sleep,
    )
}

#[cfg(windows)]
fn retry_sharing_violation(
    mut operation: impl FnMut() -> std::io::Result<()>,
    mut wait: impl FnMut(std::time::Duration),
) -> std::io::Result<()> {
    // Only retry a still-uncommitted local file replacement. Never repeat its
    // business operation, API request, or an ambiguous partially completed move.
    const BACKOFF_MS: [u64; 4] = [15, 35, 75, 125];
    for attempt in 0..=BACKOFF_MS.len() {
        match operation() {
            Err(error)
                if matches!(error.raw_os_error(), Some(32 | 33)) && attempt < BACKOFF_MS.len() =>
            {
                wait(std::time::Duration::from_millis(BACKOFF_MS[attempt]));
            }
            result => return result,
        }
    }
    unreachable!("the last attempt always returns")
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{fs::File, os::windows::fs::OpenOptionsExt, sync::mpsc, time::Duration};
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

    fn deny_delete(path: &Path) -> File {
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(path)
            .unwrap()
    }

    #[test]
    fn short_lived_windows_sharing_lock_reuses_the_same_staged_file() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("index.json");
        let source = directory.path().join("staged.tmp");
        fs::write(&target, b"old").unwrap();
        fs::write(&source, b"new").unwrap();
        let handle = deny_delete(&target);
        let (ready_sender, ready_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let holder = std::thread::spawn(move || {
            ready_sender.send(()).unwrap();
            release_receiver.recv().unwrap();
            std::thread::sleep(Duration::from_millis(40));
            drop(handle);
        });
        ready_receiver.recv().unwrap();
        release_sender.send(()).unwrap();
        atomic_replace(&source, &target).unwrap();
        holder.join().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(!source.exists());
    }

    #[test]
    fn persistent_windows_lock_reports_code_and_keeps_existing_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("index.json");
        fs::write(&target, b"old").unwrap();
        let handle = deny_delete(&target);
        let error = atomic_write(&target, b"new").unwrap_err();
        assert!(error.contains("Win32 错误码 32"), "{error}");
        assert!(!error.contains(&target.to_string_lossy().to_string()));
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        drop(handle);
    }

    #[test]
    fn ordinary_rust_read_handles_do_not_block_atomic_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("index.json");
        fs::write(&target, b"old").unwrap();
        let reader = File::open(&target).unwrap();
        atomic_write(&target, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        drop(reader);
    }

    #[test]
    fn retry_policy_is_bounded_and_never_retries_permissions_or_partial_moves() {
        for code in [5, 1176, 1177] {
            let mut calls = 0;
            let result = retry_sharing_violation(
                || {
                    calls += 1;
                    Err(std::io::Error::from_raw_os_error(code))
                },
                |_| panic!("non-sharing errors must not wait"),
            );
            assert_eq!(calls, 1);
            assert_eq!(result.unwrap_err().raw_os_error(), Some(code));
        }
        let mut calls = 0;
        let mut waited = Duration::ZERO;
        let result = retry_sharing_violation(
            || {
                calls += 1;
                Err(std::io::Error::from_raw_os_error(33))
            },
            |delay| waited += delay,
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(33));
        assert_eq!(calls, 5);
        assert_eq!(waited, Duration::from_millis(250));
        for code in [1176, 1177] {
            let error = std::io::Error::from_raw_os_error(code);
            assert!(preserve_replacement(&error));
            assert!(replacement_error(&error).contains("暂存文件已保留"));
        }
        assert!(!preserve_replacement(&std::io::Error::from_raw_os_error(
            32
        )));
    }
}
