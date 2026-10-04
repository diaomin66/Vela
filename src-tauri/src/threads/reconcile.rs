//! Explicit index reconciliation through the installed official protocol.
//! The source database is only opened read-only for an online backup here.
use super::{
    client,
    types::{ThreadReconcileResult, ThreadSource},
};
use crate::{
    core::{self, AppPaths},
    security,
};
use chrono::Utc;
use rusqlite::{
    backup::{Backup, StepResult},
    Connection, OpenFlags,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const MAX_METADATA: u64 = 256 * 1024 * 1024;

pub(super) fn run(
    paths: &AppPaths,
    source: &ThreadSource,
) -> Result<ThreadReconcileResult, String> {
    let executable = client::discover()?;
    run_with_executable(paths, source, &executable)
}

fn run_with_executable(
    paths: &AppPaths,
    source: &ThreadSource,
    executable: &Path,
) -> Result<ThreadReconcileResult, String> {
    let root = Path::new(&source.root);
    let sqlite_root = sqlite_root(paths, root);
    backup_metadata(paths, source, sqlite_root.as_deref().unwrap_or(root))?;
    let mut client = client::Client::start(executable, root, sqlite_root.as_deref())?;
    let active_count = client.list_all(false)?;
    let archived_count = client.list_all(true)?;
    Ok(ThreadReconcileResult { source_id: source.id.clone(), active_count, archived_count, completed_at: Utc::now().to_rfc3339(),
        message: format!("本机服务已核对 {active_count} 个活跃线程和 {archived_count} 个归档线程。请重新打开客户端；项目、来源或服务商筛选仍可能影响列表显示。") })
}

fn sqlite_root(paths: &AppPaths, root: &Path) -> Option<PathBuf> {
    if paths
        .config
        .parent()
        .and_then(|path| fs::canonicalize(path).ok())
        != fs::canonicalize(root).ok()
    {
        return None;
    }
    std::env::var_os("CODEX_SQLITE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn safe_file(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "无法读取线程索引文件。")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("线程索引使用了链接路径，请先检查来源目录。".into());
        }
    }
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("线程索引不是普通文件。".into());
    }
    if metadata.len() > MAX_METADATA {
        return Err("线程索引超过单次备份范围，已停止重新索引。".into());
    }
    Ok(())
}

fn backup_metadata(paths: &AppPaths, source: &ThreadSource, db_root: &Path) -> Result<(), String> {
    if !db_root.is_absolute() {
        return Err("线程数据库目录必须为绝对路径。".into());
    }
    let checkpoint = paths
        .data
        .join("threads")
        .join("metadata")
        .join(uuid::Uuid::new_v4().to_string());
    let mut artifacts = Vec::new();
    let names = Path::new(&source.root).join("session_index.jsonl");
    if names.try_exists().map_err(|_| "无法检查线程名称索引。")? {
        safe_file(&names)?;
        let before = fs::metadata(&names).map_err(|_| "无法读取名称索引状态。")?;
        let bytes = Zeroizing::new(fs::read(&names).map_err(|_| "无法备份线程名称索引。")?);
        let after = fs::metadata(&names).map_err(|_| "无法复核名称索引状态。")?;
        if before.len() != after.len()
            || before.modified().ok() != after.modified().ok()
            || bytes.len() as u64 != before.len()
        {
            return Err("线程名称正在更新，请稍后重新索引。".into());
        }
        artifacts.push(save_metadata(&checkpoint, "session_index.jsonl", &bytes)?);
    }
    if db_root
        .try_exists()
        .map_err(|_| "无法读取线程数据库目录。")?
    {
        let entries = fs::read_dir(db_root).map_err(|_| "无法列出线程数据库。")?;
        for entry in entries {
            let entry = entry.map_err(|_| "无法读取线程数据库目录项。")?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if !file_name.ends_with(".sqlite")
                || !(file_name.starts_with("state_") || file_name.starts_with("thread_history_"))
            {
                continue;
            }
            safe_file(&entry.path())?;
            let source_db = Connection::open_with_flags(
                entry.path(),
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(|_| "无法读取线程数据库，已停止重新索引。原日志保持不变。")?;
            source_db
                .busy_timeout(Duration::from_millis(500))
                .map_err(|_| "无法设置线程索引读取策略。")?;
            let page_size: u64 = source_db
                .query_row("PRAGMA page_size", [], |row| row.get::<_, u32>(0))
                .map(u64::from)
                .map_err(|_| "无法检查线程索引大小。")?;
            let page_count: u64 = source_db
                .query_row("PRAGMA page_count", [], |row| row.get::<_, u32>(0))
                .map(u64::from)
                .map_err(|_| "无法检查线程索引大小。")?;
            if page_count.saturating_mul(page_size) > MAX_METADATA {
                return Err("线程索引快照超出单次备份范围，已停止重新索引。".into());
            }
            let mut destination =
                Connection::open_in_memory().map_err(|_| "无法创建线程索引快照。")?;
            {
                let backup = Backup::new(&source_db, &mut destination)
                    .map_err(|_| "无法创建数据库在线快照。")?;
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    match backup
                        .step(256)
                        .map_err(|_| "线程数据库快照未完成，已停止重新索引。")?
                    {
                        StepResult::Done => break,
                        StepResult::More | StepResult::Busy | StepResult::Locked => {
                            if Instant::now() >= deadline {
                                return Err(
                                    "线程数据库正在使用中，无法完成一致性备份，请稍后重试。".into(),
                                );
                            }
                            if (backup.progress().pagecount as u64).saturating_mul(page_size)
                                > MAX_METADATA
                            {
                                return Err("线程索引快照超出安全大小，已停止。".into());
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        _ => return Err("线程数据库快照状态不兼容。".into()),
                    }
                }
            }
            let integrity: String = destination
                .query_row("PRAGMA quick_check", [], |row| row.get(0))
                .map_err(|_| "线程数据库完整性验证失败。")?;
            if integrity != "ok" {
                return Err("线程数据库存在损坏，已保留原文件并停止重新索引。".into());
            }
            let bytes = destination
                .serialize("main")
                .map_err(|_| "无法保存线程数据库快照。")?;
            if bytes.len() as u64 > MAX_METADATA {
                return Err("线程索引快照超出单次备份范围。".into());
            }
            artifacts.push(save_metadata(&checkpoint, &file_name, &bytes)?);
        }
    }
    core::atomic_write(&checkpoint.join("checkpoint.json"), &serde_json::to_vec_pretty(&json!({
        "version":1,"sourceId":source.id,"createdAt":Utc::now().to_rfc3339(),"artifacts":artifacts
    })).map_err(|_| "无法保存线程索引保护记录。")?)
}

fn save_metadata(
    directory: &Path,
    original_name: &str,
    bytes: &[u8],
) -> Result<serde_json::Value, String> {
    let hash = format!("{:x}", Sha256::digest(bytes));
    let encrypted = security::protect(bytes).map_err(|_| "无法加密线程索引快照。")?;
    let object_name = format!("{hash}.bin");
    core::atomic_write(&directory.join(&object_name), &encrypted)?;
    let stored = fs::read(directory.join(&object_name)).map_err(|_| "无法验证索引快照。")?;
    let verified =
        Zeroizing::new(security::unprotect(&stored).map_err(|_| "无法验证索引快照解密。")?);
    if verified.as_slice() != bytes {
        return Err("索引快照完整性校验失败，已停止重新索引。".into());
    }
    Ok(json!({"name":original_name,"object":object_name,"sha256":hash,"bytes":bytes.len()}))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn metadata_snapshot_includes_uncheckpointed_wal_and_keeps_sources_intact() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("home");
        fs::create_dir_all(&root).unwrap();
        let paths = AppPaths {
            data: directory.path().join("data"),
            config: root.join("config.toml"),
            helper: directory.path().join("unused.exe"),
        };
        let source = ThreadSource {
            id: "test-source".into(),
            kind: "local".into(),
            root: root.to_string_lossy().into_owned(),
            display_root: "test".into(),
            available: true,
            writable: true,
            last_scanned_at: None,
            error: None,
        };
        let database = root.join("state_5.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE marker(value TEXT); INSERT INTO marker VALUES('from-wal');").unwrap();
        fs::write(root.join("session_index.jsonl"), b"{\"id\":\"test\"}\n").unwrap();
        backup_metadata(&paths, &source, &root).unwrap();
        let checkpoint = fs::read_dir(paths.data.join("threads/metadata"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(checkpoint.join("checkpoint.json")).unwrap()).unwrap();
        assert_eq!(manifest["artifacts"].as_array().unwrap().len(), 2);
        let db_artifact = manifest["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == "state_5.sqlite")
            .unwrap();
        let decoded = security::unprotect(
            &fs::read(checkpoint.join(db_artifact["object"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        let test_copy = directory.path().join("verified.sqlite");
        fs::write(&test_copy, decoded).unwrap();
        let restored = Connection::open(&test_copy).unwrap();
        let value: String = restored
            .query_row("SELECT value FROM marker", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "from-wal");
        assert_eq!(
            fs::read(root.join("session_index.jsonl")).unwrap(),
            b"{\"id\":\"test\"}\n"
        );
    }
}
