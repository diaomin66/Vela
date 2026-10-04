use super::{types::*, validation};
use crate::core::{self, AppPaths};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const JOURNAL: &str = "location-migration.json";
const MAX_ENTRIES: usize = 500_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Backups,
    Evaluations,
    Exports,
    Protection,
    Index,
}
struct Job {
    key: &'static str,
    label: &'static str,
    source: PathBuf,
    target: PathBuf,
    kind: Kind,
}
#[derive(Clone)]
struct Entry {
    source: PathBuf,
    target: PathBuf,
    bytes: u64,
    sqlite: bool,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    version: u32,
    source: ResolvedLocationPaths,
    target: ResolvedLocationPaths,
    completed: u64,
    bytes: u64,
    #[serde(default)]
    required_sources: Vec<String>,
}

fn jobs(current: &ResolvedLocations, next: &ResolvedLocations) -> Vec<Job> {
    vec![
        Job {
            key: "backupsDirectory",
            label: "配置备份",
            source: current.backups_directory.clone(),
            target: next.backups_directory.clone(),
            kind: Kind::Backups,
        },
        Job {
            key: "evaluationsDirectory",
            label: "评测记录",
            source: current.evaluations_directory.clone(),
            target: next.evaluations_directory.clone(),
            kind: Kind::Evaluations,
        },
        Job {
            key: "exportsDirectory",
            label: "导出文件",
            source: current.exports_directory.clone(),
            target: next.exports_directory.clone(),
            kind: Kind::Exports,
        },
        Job {
            key: "threadProtectionDirectory",
            label: "线程保护副本",
            source: current.thread_protection_directory.clone(),
            target: next.thread_protection_directory.clone(),
            kind: Kind::Protection,
        },
        Job {
            key: "threadIndexDirectory",
            label: "线程管理索引",
            source: current.thread_index_directory.clone(),
            target: next.thread_index_directory.clone(),
            kind: Kind::Index,
        },
    ]
}

fn index_entry(name: &str) -> bool {
    name == "inventory.sqlite3" || name.starts_with("inventory-preserved-")
}
fn excluded(job: &Job, relative: &Path) -> bool {
    let Some(first) = relative
        .components()
        .next()
        .map(|part| part.as_os_str().to_string_lossy())
    else {
        return false;
    };
    match job.kind {
        Kind::Evaluations => first == "exports",
        Kind::Protection => {
            index_entry(&first)
                || first == "catalog.lock"
                || first.starts_with("inventory.sqlite3-")
                || first.starts_with("inventory-rebuild-")
        }
        Kind::Index => !index_entry(&first),
        _ => false,
    }
}

fn entries(job: &Job) -> Result<Vec<Entry>, String> {
    validation::guard(&job.source, true)?;
    validation::directory(&job.target)?;
    if !job.source.exists() {
        return Ok(Vec::new());
    }
    if !job.source.is_dir() {
        return Err("原保存位置不是目录，未迁移。".into());
    }
    let mut pending = vec![(job.source.clone(), 0u32)];
    let mut result = Vec::new();
    let mut count = 0usize;
    while let Some((folder, depth)) = pending.pop() {
        if depth > 64 {
            return Err("迁移目录层级过深，未切换位置。".into());
        }
        validation::guard(&folder, false)?;
        for entry in fs::read_dir(&folder).map_err(|_| "无法读取迁移来源目录。")? {
            let entry = entry.map_err(|_| "无法枚举迁移来源文件。")?;
            count += 1;
            if count > MAX_ENTRIES {
                return Err("迁移文件数量超过本轮安全范围，未切换位置。".into());
            }
            let path = entry.path();
            let relative = path
                .strip_prefix(&job.source)
                .map_err(|_| "迁移路径超出来源目录。")?;
            if excluded(job, relative) {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(|_| "无法读取迁移文件信息。")?;
            if validation::reparse(&metadata) {
                return Err("迁移目录包含链接或目录联接，原位置仍生效。".into());
            }
            if metadata.is_dir() {
                pending.push((path, depth + 1));
            } else if metadata.is_file() {
                let target = job.target.join(relative);
                validation::guard(&target, true)?;
                result.push(Entry {
                    sqlite: job.kind == Kind::Index && relative == Path::new("inventory.sqlite3"),
                    source: path,
                    target,
                    bytes: metadata.len(),
                });
            } else {
                return Err("迁移目录包含不支持的特殊文件。".into());
            }
        }
    }
    result.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(result)
}

fn validate_plan(current: &ResolvedLocations, next: &ResolvedLocations) -> Result<(), String> {
    let all = jobs(current, next);
    for job in &all {
        if validation::same(&job.source, &job.target)? {
            continue;
        }
        for source in &all {
            // Exports and the thread inventory have explicitly disjoint ownership
            // inside their default shared directories. Allow returning to those
            // layouts when the other owner is staying in place.
            let shared_owner_stays = validation::same(&source.source, &source.target)?
                && match (job.kind, source.kind) {
                    (Kind::Exports, Kind::Evaluations) => {
                        validation::same(&job.target, &source.source.join("exports"))?
                    }
                    (Kind::Evaluations, Kind::Exports) => {
                        validation::same(&source.source, &job.target.join("exports"))?
                    }
                    (Kind::Index, Kind::Protection) | (Kind::Protection, Kind::Index) => {
                        validation::same(&job.target, &source.source)?
                    }
                    _ => false,
                };
            if shared_owner_stays {
                continue;
            }
            if validation::overlaps(&job.target, &source.source)? {
                return Err("新保存目录不能覆盖或嵌套旧保存目录，请选择独立空目录。".into());
            }
        }
    }
    Ok(())
}

pub(super) fn preview(
    current: &ResolvedLocations,
    next: &ResolvedLocations,
) -> Result<Vec<LocationChange>, String> {
    validate_plan(current, next)?;
    let mut changes = Vec::new();
    for (key, label, source, target) in [
        (
            "codexHome",
            "官方配置与会话",
            &current.codex_home,
            &next.codex_home,
        ),
        (
            "sqliteHome",
            "官方 SQLite 索引",
            &current.sqlite_home,
            &next.sqlite_home,
        ),
    ] {
        if !validation::same(source, target)? {
            changes.push(LocationChange {
                key: key.into(),
                label: label.into(),
                current_path: validation::display(source),
                next_path: validation::display(target),
                migration: "switch".into(),
                files: 0,
                bytes: 0,
            });
        }
    }
    for job in jobs(current, next) {
        if validation::same(&job.source, &job.target)? {
            continue;
        }
        let files = entries(&job)?;
        let bytes = files
            .iter()
            .try_fold(0u64, |sum, entry| sum.checked_add(entry.bytes))
            .ok_or("迁移容量超出可计算范围。")?;
        for entry in &files {
            if entry.target.exists() && !entry.sqlite {
                if fs::metadata(&entry.target).map(|meta| meta.len()).ok() != Some(entry.bytes)
                    || hash_file(&entry.source)? != hash_file(&entry.target)?
                {
                    return Err(format!(
                        "{}目标中存在不同内容，未覆盖：{}",
                        job.label,
                        validation::display(&entry.target)
                    ));
                }
            }
        }
        changes.push(LocationChange {
            key: job.key.into(),
            label: job.label.into(),
            current_path: validation::display(&job.source),
            next_path: validation::display(&job.target),
            migration: "copy".into(),
            files: files.len() as u64,
            bytes,
        });
    }
    Ok(changes)
}

pub(super) fn in_progress(paths: &AppPaths) -> Result<bool, String> {
    validation::guard(&paths.data.join(JOURNAL), true)?;
    Ok(paths.data.join(JOURNAL).exists())
}
fn journal(paths: &AppPaths, value: &Journal) -> Result<(), String> {
    core::atomic_write(
        &paths.data.join(JOURNAL),
        &serde_json::to_vec(value).map_err(|_| "无法记录位置迁移事务。")?,
    )
}

fn read_journal(paths: &AppPaths) -> Result<Option<Journal>, String> {
    if !in_progress(paths)? {
        return Ok(None);
    }
    let source = paths.data.join(JOURNAL);
    if fs::metadata(&source)
        .map_err(|_| "无法读取位置迁移事务。")?
        .len()
        > 256 * 1024
    {
        return Err("位置迁移事务大小异常。".into());
    }
    let raw = fs::read(source).map_err(|_| "无法读取上次位置迁移事务。")?;
    let value: Journal =
        serde_json::from_slice(&raw).map_err(|_| "位置迁移事务损坏，原数据仍保留。")?;
    if value.version != 1 {
        return Err("位置迁移事务版本不兼容。".into());
    }
    Ok(Some(value))
}

pub(super) fn abandon(paths: &AppPaths) -> Result<(), String> {
    if in_progress(paths)? {
        let archived = paths.data.join(format!(
            "location-migration-abandoned-{}.json",
            uuid::Uuid::new_v4()
        ));
        publish(&paths.data.join(JOURNAL), &archived)?;
    }
    Ok(())
}

pub(super) fn require_sources(sources: &[String]) -> Result<(), String> {
    for source in sources {
        let source = Path::new(source);
        validation::guard(source, false)?;
        if !source.is_dir() {
            return Err(
                "已确认的迁移来源目录不再可用，未切换位置。请重新连接原磁盘或取消待重启方案。"
                    .into(),
            );
        }
    }
    Ok(())
}

pub(super) fn finish_committed(paths: &AppPaths, active: &ResolvedLocations) -> Result<(), String> {
    if let Some(journal) = read_journal(paths)? {
        if journal.target != active.display() {
            return Err(
                "存在未完成的位置迁移记录；可重新预览并保存位置设置，原目录仍生效。".into(),
            );
        }
        finish(paths)?;
    }
    Ok(())
}

pub(super) fn execute(
    paths: &AppPaths,
    current: &ResolvedLocations,
    next: &ResolvedLocations,
) -> Result<(), String> {
    validate_plan(current, next)?;
    let mut record = Journal {
        version: 1,
        source: current.display(),
        target: next.display(),
        completed: 0,
        bytes: 0,
        required_sources: jobs(current, next)
            .iter()
            .filter(|job| job.source.is_dir())
            .map(|job| validation::display(&job.source))
            .collect(),
    };
    if let Some(old) = read_journal(paths)? {
        if old.source != record.source || old.target != record.target {
            return Err("上次迁移与当前位置不一致，原位置仍生效。".into());
        }
        require_sources(&old.required_sources)?;
    }
    require_sources(&record.required_sources)?;
    let mut locks = Vec::new();
    let mut locked = HashSet::new();
    for directory in [
        &current.thread_index_directory,
        &next.thread_index_directory,
    ] {
        if !directory.exists() {
            continue;
        }
        let lock_path = directory.join("catalog.lock");
        if !locked.insert(validation::normalized(&lock_path)?) {
            continue;
        }
        validation::guard(&lock_path, true)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| "无法锁定线程索引迁移。")?;
        file.try_lock_exclusive()
            .map_err(|_| "线程索引仍在使用，原位置保持生效，请关闭其他实例后重试。")?;
        locks.push(file);
    }
    journal(paths, &record)?;
    for job in jobs(current, next) {
        if validation::same(&job.source, &job.target)? {
            continue;
        }
        for entry in entries(&job)? {
            copy(&entry)?;
            record.completed += 1;
            record.bytes = record.bytes.saturating_add(entry.bytes);
            journal(paths, &record)?;
        }
    }
    require_sources(&record.required_sources)?;
    Ok(())
}

pub(super) fn finish(paths: &AppPaths) -> Result<(), String> {
    validation::guard(&paths.data.join(JOURNAL), true)?;
    match fs::remove_file(paths.data.join(JOURNAL)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("位置已切换，但迁移记录暂时无法清理。".into()),
    }
}

fn source_file(path: &Path) -> Result<File, String> {
    validation::guard(path, false)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    }
    options
        .open(path)
        .map_err(|_| "迁移来源正在写入或无法读取，请关闭相关程序后重试。".into())
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = source_file(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let length = file
            .read(&mut buffer)
            .map_err(|_| "读取迁移校验内容失败。")?;
        if length == 0 {
            break;
        }
        digest.update(&buffer[..length]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn copy(entry: &Entry) -> Result<(), String> {
    validation::guard(&entry.target, true)?;
    let parent = entry.target.parent().ok_or("迁移目标无效。")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建迁移目标目录。")?;
    validation::guard(parent, false)?;
    let staging = parent.join(format!(".ahax-location-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        if entry.sqlite {
            sqlite_snapshot(&entry.source, &staging)?;
        } else {
            let mut source = source_file(&entry.source)?;
            let before = source.metadata().map_err(|_| "无法读取来源文件信息。")?;
            let mut target = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&staging)
                .map_err(|_| "无法暂存迁移文件。")?;
            let copied = std::io::copy(&mut source, &mut target)
                .map_err(|_| "复制迁移文件失败，原文件仍保留。")?;
            target
                .flush()
                .and_then(|_| target.sync_all())
                .map_err(|_| "迁移文件未完整落盘。")?;
            drop(target);
            let after = source.metadata().map_err(|_| "无法核验来源文件信息。")?;
            if copied != before.len()
                || before.len() != after.len()
                || before.modified().ok() != after.modified().ok()
            {
                return Err("迁移期间来源文件发生变化，原位置仍生效。".into());
            }
            drop(source);
            if hash_file(&entry.source)? != hash_file(&staging)? {
                return Err("迁移副本校验失败，原文件仍保留。".into());
            }
        }
        if entry.target.exists() {
            if hash_file(&staging)? == hash_file(&entry.target)? {
                fs::remove_file(&staging).map_err(|_| "无法清理已验证迁移暂存文件。")?;
                return Ok(());
            }
            return Err(format!(
                "迁移目标已有不同内容，未覆盖：{}",
                validation::display(&entry.target)
            ));
        }
        publish(&staging, &entry.target)?;
        Ok(())
    })();
    if result.is_err() && staging.exists() {
        let _ = fs::remove_file(staging);
    }
    result
}

fn sqlite_snapshot(source: &Path, target: &Path) -> Result<(), String> {
    validation::guard(source, false)?;
    let original =
        rusqlite::Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| "线程索引无法读取，原位置保持生效。")?;
    original
        .busy_timeout(Duration::from_secs(1))
        .map_err(|_| "无法设置索引备份等待时间。")?;
    let mut destination = rusqlite::Connection::open(target).map_err(|_| "无法暂存线程索引。")?;
    let deadline = Instant::now() + Duration::from_secs(120);
    {
        let backup = rusqlite::backup::Backup::new(&original, &mut destination)
            .map_err(|_| "无法创建一致性线程索引副本。")?;
        loop {
            match backup.step(256).map_err(|_| "线程索引复制未完成。")? {
                rusqlite::backup::StepResult::Done => break,
                _ => {
                    if Instant::now() > deadline {
                        return Err("线程索引长时间占用，未切换保存位置。".into());
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
    let check: String = destination
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|_| "线程索引副本无法验证。")?;
    if check != "ok" {
        return Err("线程索引副本完整性检查失败。".into());
    }
    drop(destination);
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(target)
        .and_then(|file| file.sync_all())
        .map_err(|_| "线程索引副本未完整落盘。")?;
    Ok(())
}

fn publish(source: &Path, target: &Path) -> Result<(), String> {
    validation::guard(target, true)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        // Rust's filesystem calls support long Windows paths, while raw Win32
        // calls still require the extended-length spelling. Canonicalize the
        // existing source and destination parent through std so both drive and
        // UNC paths receive that native spelling without truncating hashes or
        // requiring global long-path policy changes.
        let source = fs::canonicalize(source).map_err(|_| "无法确认迁移暂存文件的位置。")?;
        let parent = target.parent().ok_or("迁移发布目标无效。")?;
        let name = target.file_name().ok_or("迁移发布文件名无效。")?;
        let target = fs::canonicalize(parent)
            .map_err(|_| "无法确认迁移目标目录的位置。")?
            .join(name);
        let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            let error = std::io::Error::last_os_error();
            return Err(format!(
                "迁移文件无法发布（Windows 错误码 {}）；已有内容未被覆盖。",
                error.raw_os_error().unwrap_or(0)
            ));
        }
    }
    #[cfg(not(windows))]
    {
        fs::hard_link(source, target).map_err(|_| "无法发布迁移文件，目标未覆盖。")?;
        fs::remove_file(source).map_err(|_| "无法清理迁移暂存文件。")?;
    }
    Ok(())
}
