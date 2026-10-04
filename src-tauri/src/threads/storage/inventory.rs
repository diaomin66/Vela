use super::{
    paths::{self, directory, index_directory},
    vault,
};
use crate::{
    core::{self, AppPaths},
    security,
    threads::types::*,
};
use fs2::FileExt;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    path::Path,
    time::Duration,
};
use zeroize::Zeroizing;

const SCHEMA_VERSION: i64 = 1;

fn lock(paths: &AppPaths) -> Result<File, String> {
    let folder = index_directory(paths);
    paths::ensure_directory(&folder)?;
    let path = folder.join("catalog.lock");
    paths::guard_path(&path, true)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|_| "无法锁定线程索引。")?;
    file.try_lock_exclusive()
        .map_err(|_| "线程索引正在更新，请稍后重试。")?;
    Ok(file)
}

fn open_at(path: &Path, writable: bool) -> Result<Connection, String> {
    paths::guard_path(path, writable)?;
    let flags = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection = Connection::open_with_flags(path, flags)
        .map_err(|_| "线程索引无法打开；保护副本仍保留，请重建索引。")?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|_| "无法设置线程索引等待时间。")?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| "线程索引损坏；保护副本仍保留，请重建索引。")?;
    if version > SCHEMA_VERSION {
        return Err("线程索引来自较新版本，请更新应用。".into());
    }
    if writable {
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY;
            CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS sources (id TEXT PRIMARY KEY, root TEXT NOT NULL, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS threads (key TEXT PRIMARY KEY, source_id TEXT NOT NULL, thread_id TEXT NOT NULL, relative_path TEXT NOT NULL, title TEXT, cwd TEXT, updated_at TEXT, integrity TEXT NOT NULL, recoverability TEXT NOT NULL, body TEXT NOT NULL, UNIQUE(source_id, relative_path));
            CREATE INDEX IF NOT EXISTS threads_source_time ON threads(source_id, updated_at DESC, key);
            CREATE INDEX IF NOT EXISTS threads_identity ON threads(source_id, thread_id);
            PRAGMA user_version=1;").map_err(|_| "无法初始化线程索引；原有文件保持不变。")?;
    }
    Ok(connection)
}

pub(in crate::threads) fn read(paths: &AppPaths) -> Result<ThreadIndex, String> {
    super::delete::tombstones(paths)?;
    let path = index_directory(paths).join("inventory.sqlite3");
    if !path.exists() {
        return Ok(ThreadIndex::default());
    }
    let connection = open_at(&path, false)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|_| "无法读取线程索引快照。")?;
    let text: Option<String> = transaction
        .query_row("SELECT value FROM metadata WHERE key='index'", [], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|_| "线程索引损坏；保护副本仍保留，请重建索引。")?;
    let mut index: ThreadIndex = text
        .map(|text| serde_json::from_str(&text).map_err(|_| "线程索引元数据无法解析。".to_owned()))
        .transpose()?
        .unwrap_or_default();
    {
        let mut statement = transaction
            .prepare("SELECT body FROM sources ORDER BY id")
            .map_err(|_| "无法读取线程来源索引。")?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|_| "无法读取线程来源索引。")?;
        for row in rows {
            index.sources.push(
                serde_json::from_str(&row.map_err(|_| "线程来源索引读取失败。")?)
                    .map_err(|_| "线程来源索引内容无效。")?,
            );
        }
    }
    {
        let mut statement = transaction
            .prepare("SELECT body FROM threads ORDER BY updated_at DESC, key")
            .map_err(|_| "无法读取线程清单。")?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|_| "无法读取线程清单。")?;
        for row in rows {
            index.threads.push(
                serde_json::from_str(&row.map_err(|_| "线程清单读取失败。")?)
                    .map_err(|_| "线程清单内容无效。")?,
            );
        }
    }
    transaction.commit().map_err(|_| "无法完成线程索引读取。")?;
    filter_deleted(paths, &mut index)?;
    Ok(index)
}


fn filter_deleted(paths: &AppPaths, index: &mut ThreadIndex) -> Result<(), String> {
    let tombstones = super::delete::tombstones(paths)?;
    index.threads.retain(|thread| !tombstones.contains(&(thread.source_id.clone(), thread.thread_id.clone())));
    index.protection.protected_count = index.threads.iter().filter(|thread| thread.snapshot == SnapshotState::Protected).count() as u64;
    index.protection.pending_count = index.threads.iter().filter(|thread| thread.snapshot == SnapshotState::Pending).count() as u64;
    index.protection.failed_count = index.threads.iter().filter(|thread| matches!(thread.snapshot, SnapshotState::Failed | SnapshotState::Missing | SnapshotState::TooLarge)).count() as u64;
    index.protection.bytes_protected = index.threads.iter().filter_map(|thread| thread.protected_bytes).sum();
    Ok(())
}

fn metadata(connection: &Connection) -> Result<ThreadIndex, String> {
    let text: Option<String> = connection
        .query_row("SELECT value FROM metadata WHERE key='index'", [], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|_| "线程索引损坏；保护副本仍保留，请重建索引。")?;
    text.map(|text| serde_json::from_str(&text).map_err(|_| "线程保护状态无法解析。".to_owned()))
        .transpose()
        .map(|index| index.unwrap_or_default())
}

pub(in crate::threads) fn read_state(paths: &AppPaths) -> Result<ThreadIndex, String> {
    super::delete::tombstones(paths)?;
    let path = index_directory(paths).join("inventory.sqlite3");
    if !path.exists() {
        return Ok(ThreadIndex::default());
    }
    metadata(&open_at(&path, false)?)
}

pub(in crate::threads) fn lookup(
    paths: &AppPaths,
    key: &str,
) -> Result<Option<ThreadSummary>, String> {
    let hidden = super::delete::tombstones(paths)?;
    if key.is_empty() || key.len() > 256 {
        return Err("线程标识无效。".into());
    }
    let path = index_directory(paths).join("inventory.sqlite3");
    if !path.exists() {
        return Ok(None);
    }
    let connection = open_at(&path, false)?;
    let row: Option<String> = connection
        .query_row("SELECT body FROM threads WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|_| "无法读取所选线程。")?;
    let thread: Option<ThreadSummary> = row.map(|text| serde_json::from_str(&text).map_err(|_| "线程记录无法解析。".to_owned())).transpose()?;
    Ok(thread.filter(|thread| !hidden.contains(&(thread.source_id.clone(), thread.thread_id.clone()))))
}

pub(in crate::threads) fn overview(paths: &AppPaths) -> Result<ThreadDashboard, String> {
    let hidden = super::delete::tombstones(paths)?;
    let path = index_directory(paths).join("inventory.sqlite3");
    let mut index = ThreadIndex::default();
    let mut counts = (0u64, 0u64, 0u64, 0u64);
    if path.exists() {
        let connection = open_at(&path, false)?;
        attach_hidden(&connection, &hidden)?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|_| "无法读取线程保护概览。")?;
        index = metadata(&transaction)?;
        {
            let mut statement = transaction
                .prepare("SELECT body FROM sources ORDER BY id")
                .map_err(|_| "无法读取线程来源。")?;
            for row in statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|_| "无法读取线程来源。")?
            {
                index.sources.push(
                    serde_json::from_str(&row.map_err(|_| "线程来源读取失败。")?)
                        .map_err(|_| "线程来源内容无效。")?,
                );
            }
        }
        let raw: (i64,i64,i64,i64) = transaction.query_row("SELECT COUNT(*), COALESCE(SUM(json_extract(body,'$.snapshot')='protected'),0), COALESCE(SUM(recoverability='recoverable'),0), COALESCE(SUM(integrity!='valid' OR json_extract(body,'$.snapshot')!='protected' OR json_extract(body,'$.stateIndex')='missing'),0) FROM threads WHERE NOT EXISTS(SELECT 1 FROM hidden_threads h WHERE h.source_id=threads.source_id AND h.thread_id=threads.thread_id)", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).map_err(|_| "无法读取线程保护统计。")?;
        counts = (
            unsigned(raw.0)?,
            unsigned(raw.1)?,
            unsigned(raw.2)?,
            unsigned(raw.3)?,
        );
        transaction
            .commit()
            .map_err(|_| "无法完成线程保护概览读取。")?;
    }
    let error = index.protection.error.clone();
    Ok(ThreadDashboard {
        sources: index.sources,
        threads: Vec::new(),
        protection: index.protection,
        scanned_at: index.last_scan_at,
        scan_revision: index.scan_revision,
        total: counts.0,
        protected: counts.1,
        recoverable: counts.2,
        attention: counts.3,
        error,
    })
}

pub(in crate::threads) fn page(
    paths: &AppPaths,
    query: &ThreadListQuery,
) -> Result<ThreadPage, String> {
    let hidden = super::delete::tombstones(paths)?;
    if query.limit == 0 || query.limit > 100 || query.search.chars().count() > 200 {
        return Err("每页显示数量需为 1–100 条，搜索内容不能超过 200 字。".into());
    }
    if !matches!(query.scope.as_str(), "all" | "active" | "archived")
        || !matches!(
            query.status.as_str(),
            "all" | "protected" | "recoverable" | "attention"
        )
    {
        return Err("线程筛选条件无效。".into());
    }
    if let Some(source) = &query.source_id {
        paths::safe_id(source)?;
    }
    let path = index_directory(paths).join("inventory.sqlite3");
    let mut result = ThreadPage {
        threads: Vec::new(),
        total: 0,
        offset: query.offset,
        limit: query.limit,
        scan_revision: String::new(),
    };
    if !path.exists() {
        return Ok(result);
    }
    let connection = open_at(&path, false)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|_| "无法读取线程列表。")?;
    attach_hidden(&transaction, &hidden)?;
    result.scan_revision = metadata(&transaction)?.scan_revision;
    let search = format!(
        "%{}%",
        query
            .search
            .trim()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let filter = "NOT EXISTS(SELECT 1 FROM hidden_threads h WHERE h.source_id=threads.source_id AND h.thread_id=threads.thread_id) AND (?1 IS NULL OR source_id=?1) AND (?2='all' OR (?2='active' AND json_extract(body,'$.archived')=0) OR (?2='archived' AND json_extract(body,'$.archived')=1)) AND (?3='all' OR (?3='protected' AND json_extract(body,'$.snapshot')='protected') OR (?3='recoverable' AND recoverability='recoverable') OR (?3='attention' AND (integrity!='valid' OR json_extract(body,'$.snapshot')!='protected' OR json_extract(body,'$.stateIndex')='missing'))) AND (?4='%%' OR COALESCE(title,'') LIKE ?4 ESCAPE '\\' OR COALESCE(cwd,'') LIKE ?4 ESCAPE '\\' OR thread_id LIKE ?4 ESCAPE '\\')";
    let count: i64 = transaction
        .query_row(
            &format!("SELECT COUNT(*) FROM threads WHERE {filter}"),
            params![query.source_id, query.scope, query.status, search],
            |row| row.get(0),
        )
        .map_err(|_| "无法统计筛选后的线程。")?;
    result.total = unsigned(count)?;
    {
        let mut statement = transaction.prepare(&format!("SELECT body FROM threads WHERE {filter} ORDER BY updated_at DESC,key LIMIT ?5 OFFSET ?6")).map_err(|_| "无法准备线程分页。")?;
        for row in statement
            .query_map(
                params![
                    query.source_id,
                    query.scope,
                    query.status,
                    search,
                    query.limit,
                    query.offset
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|_| "无法读取线程分页。")?
        {
            result.threads.push(
                serde_json::from_str(&row.map_err(|_| "线程分页读取失败。")?)
                    .map_err(|_| "线程记录格式无效。")?,
            );
        }
    }
    transaction.commit().map_err(|_| "无法完成线程分页读取。")?;
    Ok(result)
}

fn attach_hidden(connection: &Connection, hidden: &std::collections::HashSet<(String, String)>) -> Result<(), String> {
    connection.execute_batch("CREATE TEMP TABLE hidden_threads(source_id TEXT NOT NULL, thread_id TEXT NOT NULL, PRIMARY KEY(source_id,thread_id));").map_err(|_| "无法过滤线程回收站。")?;
    let mut insert = connection.prepare("INSERT INTO hidden_threads VALUES(?1,?2)").map_err(|_| "无法准备回收站过滤。")?;
    for (source, id) in hidden { insert.execute(params![source,id]).map_err(|_| "无法加载回收站删除状态。")?; }
    Ok(())
}

fn unsigned(value: i64) -> Result<u64, String> {
    u64::try_from(value).map_err(|_| "线程索引包含无效统计值。".into())
}

fn write_connection(connection: &mut Connection, index: &ThreadIndex) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|_| "无法开始线程索引更新。")?;
    transaction.execute_batch("CREATE TEMP TABLE IF NOT EXISTS current_thread_keys (key TEXT PRIMARY KEY); DELETE FROM current_thread_keys;").map_err(|_| "无法准备线程清单替换。")?;
    let mut metadata = index.clone();
    metadata.threads.clear();
    metadata.sources.clear();
    let body = serde_json::to_string(&metadata).map_err(|_| "无法编码线程保护状态。")?;
    transaction.execute("INSERT INTO metadata(key,value) VALUES('index',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [body]).map_err(|_| "无法保存线程保护状态。")?;
    {
        let mut insert = transaction.prepare("INSERT INTO sources(id,root,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET root=excluded.root,body=excluded.body").map_err(|_| "无法准备线程来源更新。")?;
        for source in &index.sources {
            paths::safe_id(&source.id)?;
            paths::normalized(Path::new(&source.root))?;
            let body = serde_json::to_string(source).map_err(|_| "无法编码线程来源。")?;
            insert
                .execute(params![source.id, source.root, body])
                .map_err(|_| "无法保存线程来源。")?;
        }
    }
    {
        let mut insert = transaction.prepare("INSERT INTO threads(key,source_id,thread_id,relative_path,title,cwd,updated_at,integrity,recoverability,body) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(key) DO UPDATE SET source_id=excluded.source_id,thread_id=excluded.thread_id,relative_path=excluded.relative_path,title=excluded.title,cwd=excluded.cwd,updated_at=excluded.updated_at,integrity=excluded.integrity,recoverability=excluded.recoverability,body=excluded.body").map_err(|_| "无法准备线程清单更新。")?;
        for thread in &index.threads {
            let body = serde_json::to_string(thread).map_err(|_| "无法编码线程清单。")?;
            insert
                .execute(params![
                    thread.key,
                    thread.source_id,
                    thread.thread_id,
                    thread.relative_path,
                    thread.title,
                    thread.cwd,
                    thread.updated_at,
                    thread.integrity,
                    thread.recoverability,
                    body
                ])
                .map_err(|_| "无法保存线程清单；已有快照仍完整保留。")?;
            transaction
                .execute(
                    "INSERT INTO current_thread_keys(key) VALUES(?1)",
                    [&thread.key],
                )
                .map_err(|_| "线程清单包含重复记录。")?;
        }
    }
    transaction
        .execute(
            "DELETE FROM threads WHERE key NOT IN (SELECT key FROM current_thread_keys)",
            [],
        )
        .map_err(|_| "无法提交当前线程清单。")?;
    transaction
        .commit()
        .map_err(|_| "线程索引未能提交；已有快照仍完整保留。".into())
}

pub(in crate::threads) fn write(paths: &AppPaths, index: &ThreadIndex) -> Result<(), String> {
    let mut filtered = index.clone();
    filter_deleted(paths, &mut filtered)?;
    let index = &filtered;
    paths::ensure_directory(&directory(paths))?;
    let _lock = lock(paths)?;
    let mut connection = open_at(&index_directory(paths).join("inventory.sqlite3"), true)?;
    let mut catalog = index.clone();
    catalog.threads.clear();
    let raw = Zeroizing::new(serde_json::to_vec(&catalog).map_err(|_| "无法保存线程来源副本。")?);
    let encrypted = security::protect(&raw)?;
    let settings_path = directory(paths).join("catalog.bin");
    paths::guard_path(&settings_path, true)?;
    core::atomic_write(&settings_path, &encrypted)?;
    write_connection(&mut connection, index)
}

fn from_manifests(paths: &AppPaths) -> Result<ThreadIndex, String> {
    let tombstones = super::delete::tombstones(paths)?;
    let mut index = ThreadIndex::default();
    let mut warnings = 0u64;
    let catalog = directory(paths).join("catalog.bin");
    if catalog.exists() {
        let loaded = (|| {
            paths::guard_path(&catalog, false)?;
            let encrypted = fs::read(catalog).map_err(|_| "无法读取线程来源副本。")?;
            let raw = Zeroizing::new(security::unprotect(&encrypted)?);
            serde_json::from_slice(&raw).map_err(|_| "线程来源副本无法解析。".to_owned())
        })();
        match loaded {
            Ok(value) => index = value,
            Err(_) => warnings += 1,
        }
    }
    index.threads.clear();
    let root = directory(paths).join("manifests");
    if !root.exists() {
        return Ok(index);
    }
    let mut pending = vec![root];
    let mut recovered: HashMap<String, vault::Manifest> = HashMap::new();
    let mut broken: HashMap<String, vault::Manifest> = HashMap::new();
    while let Some(folder) = pending.pop() {
        if paths::guard_path(&folder, false).is_err() {
            warnings += 1;
            continue;
        }
        let entries = match fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(_) => {
                warnings += 1;
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    warnings += 1;
                    continue;
                }
            };
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(_) => {
                    warnings += 1;
                    continue;
                }
            };
            if paths::reparse(&metadata) {
                warnings += 1;
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
                continue;
            }
            if entry.path().extension().and_then(|value| value.to_str()) != Some("bin") {
                continue;
            }
            let manifest = match vault::load_manifest(&entry.path()) {
                Ok(manifest) => manifest,
                Err(_) => {
                    warnings += 1;
                    continue;
                }
            };
            if tombstones.contains(&(manifest.thread.source_id.clone(), manifest.thread.thread_id.clone())) { continue; }
            if vault::manifest_path(paths, &manifest)? != entry.path() {
                warnings += 1;
                continue;
            }
            if vault::verify(paths, &manifest).is_err() {
                warnings += 1;
                broken.insert(manifest.thread.key.clone(), manifest);
                continue;
            }
            let replace = recovered.get(&manifest.thread.key).is_none_or(|previous| {
                let valid = manifest.thread.integrity == "valid";
                let old_valid = previous.thread.integrity == "valid";
                valid && !old_valid
                    || valid == old_valid && manifest.captured_at > previous.captured_at
            });
            if replace {
                recovered.insert(manifest.thread.key.clone(), manifest);
            }
        }
    }
    let recovered_keys: std::collections::HashSet<_> = recovered.keys().cloned().collect();
    let manifests = recovered
        .into_values()
        .map(|manifest| (manifest, true))
        .chain(
            broken
                .into_iter()
                .filter(|(key, _)| !recovered_keys.contains(key))
                .map(|(_, manifest)| (manifest, false)),
        );
    for (manifest, verified) in manifests {
        if !index
            .sources
            .iter()
            .any(|source| source.id == manifest.thread.source_id)
        {
            index.sources.push(ThreadSource {
                id: manifest.thread.source_id.clone(),
                kind: "recovered".into(),
                root: manifest.source_root.clone(),
                sqlite_home: None,
                display_root: manifest.source_root,
                available: false,
                writable: false,
                last_scanned_at: None,
                error: Some("来源已从保护副本重建，请重新扫描确认当前状态。".into()),
            });
        }
        let mut thread = manifest.thread;
        thread.snapshot = if verified && thread.integrity == "valid" {
            SnapshotState::Protected
        } else {
            SnapshotState::Failed
        };
        thread.protected_bytes = verified.then_some(manifest.bytes);
        thread.last_verified_at = verified.then_some(manifest.captured_at);
        thread.recoverability = if !verified {
            "unavailable"
        } else if thread.integrity == "valid" {
            "recoverable"
        } else {
            "unsupported"
        }
        .into();
        thread.integrity = "missing".into();
        thread.scan_revision.clear();
        index.threads.push(thread);
    }
    index.protection.protected_count = index
        .threads
        .iter()
        .filter(|thread| thread.snapshot == SnapshotState::Protected)
        .count() as u64;
    index.protection.failed_count = index
        .threads
        .iter()
        .filter(|thread| thread.snapshot == SnapshotState::Failed)
        .count() as u64;
    index.protection.bytes_protected = index
        .threads
        .iter()
        .filter_map(|thread| thread.protected_bytes)
        .sum();
    index.protection.state = "idle".into();
    index.protection.current_path = None;
    index.protection.error = Some(if warnings > 0 {
        format!("线程索引已重建；{warnings} 项副本或目录无法校验，已保留证据，其余有效线程已找回。")
    } else {
        "线程索引已由加密快照重建，请重新扫描确认源文件。".into()
    });
    index.last_scan_at = None;
    index.scan_revision = uuid::Uuid::new_v4().to_string();
    Ok(index)
}

pub(in crate::threads) fn recover(paths: &AppPaths) -> Result<(), String> {
    super::delete::audit(paths)?;
    let root = index_directory(paths);
    if !root.exists() && !directory(paths).exists() { return Ok(()); }
    paths::ensure_directory(&root)?;
    paths::guard_path(&root, false)?;
    let database = root.join("inventory.sqlite3");
    if database.exists() {
        let connection = open_at(&database, false)?;
        let result: String = connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(|_| "线程索引损坏；加密快照未受影响，请重建索引。")?;
        if result != "ok" {
            return Err("线程索引完整性检查未通过；加密快照未受影响，请重建索引。".into());
        }
        drop(connection);
        merge_uncommitted_manifests(paths)?;
        let index = read(paths)?;
        write(paths, &index)?;
        return audit_warning(paths);
    }
    let _lock = lock(paths)?;
    let index = from_manifests(paths)?;
    let mut connection = open_at(&database, true)?;
    write_connection(&mut connection, &index)?;
    drop(connection);
    drop(_lock);
    audit_warning(paths)
}

fn audit_warning(paths: &AppPaths) -> Result<(), String> {
    if let Err(warning) = super::restore::audit(paths) {
        let mut index = read(paths)?;
        index.protection.error = Some(match index.protection.error {
            Some(previous) if previous.contains(&warning) => previous,
            Some(previous) => format!("{previous} {warning}"),
            None => warning,
        });
        write(paths, &index)?;
    }
    Ok(())
}

fn merge_uncommitted_manifests(paths: &AppPaths) -> Result<(), String> {
    let tombstones = super::delete::tombstones(paths)?;
    let manifest_root = directory(paths).join("manifests");
    if !manifest_root.exists() {
        return Ok(());
    }
    let mut index = read(paths)?;
    let positions: HashMap<_, _> = index
        .threads
        .iter()
        .enumerate()
        .map(|(position, thread)| (thread.key.clone(), position))
        .collect();
    let mut pending = vec![manifest_root];
    let mut candidates: HashMap<String, vault::Manifest> = HashMap::new();
    let mut warnings = 0u64;
    while let Some(folder) = pending.pop() {
        if paths::guard_path(&folder, false).is_err() {
            warnings += 1;
            continue;
        }
        let entries = match fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(_) => {
                warnings += 1;
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    warnings += 1;
                    continue;
                }
            };
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(_) => {
                    warnings += 1;
                    continue;
                }
            };
            if paths::reparse(&metadata) {
                warnings += 1;
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
                continue;
            }
            if entry.path().extension().and_then(|value| value.to_str()) != Some("bin") {
                continue;
            }
            let manifest = match vault::load_manifest(&entry.path()) {
                Ok(manifest) => manifest,
                Err(_) => {
                    warnings += 1;
                    continue;
                }
            };
            if tombstones.contains(&(manifest.thread.source_id.clone(), manifest.thread.thread_id.clone())) { continue; }
            if vault::manifest_path(paths, &manifest)? != entry.path() {
                warnings += 1;
                continue;
            }
            if let Some(position) = positions.get(&manifest.thread.key) {
                let known = &index.threads[*position];
                if known.fingerprint == manifest.hash
                    || (known.scan_revision == manifest.thread.scan_revision
                        && known.protected_bytes == Some(manifest.bytes))
                    || known
                        .last_verified_at
                        .as_ref()
                        .is_some_and(|time| time >= &manifest.captured_at)
                {
                    continue;
                }
                if known.updated_at > manifest.thread.updated_at {
                    continue;
                }
            } else if index.threads.iter().any(|known| {
                known.source_id == manifest.thread.source_id
                    && known.thread_id == manifest.thread.thread_id
                    && known.fingerprint == manifest.hash
                    && known.integrity != "missing"
            }) {
                continue;
            }
            if vault::verify(paths, &manifest).is_err() {
                warnings += 1;
                continue;
            }
            let replace = candidates.get(&manifest.thread.key).is_none_or(|current| {
                let valid = manifest.thread.integrity == "valid";
                let current_valid = current.thread.integrity == "valid";
                valid && !current_valid
                    || valid == current_valid && manifest.captured_at > current.captured_at
            });
            if replace {
                candidates.insert(manifest.thread.key.clone(), manifest);
            }
        }
    }
    if candidates.is_empty() && warnings == 0 {
        return Ok(());
    }
    let recovered_count = candidates.len();
    for manifest in candidates.into_values() {
        if !index
            .sources
            .iter()
            .any(|source| source.id == manifest.thread.source_id)
        {
            index.sources.push(ThreadSource {
                id: manifest.thread.source_id.clone(),
                kind: "recovered".into(),
                root: manifest.source_root.clone(),
                sqlite_home: None,
                display_root: manifest.source_root,
                available: false,
                writable: false,
                last_scanned_at: None,
                error: Some("已找回未提交索引的保护副本，请重新扫描。".into()),
            });
        }
        let mut thread = manifest.thread;
        thread.snapshot = if thread.integrity == "valid" {
            SnapshotState::Protected
        } else {
            SnapshotState::Failed
        };
        thread.protected_bytes = Some(manifest.bytes);
        thread.last_verified_at = Some(manifest.captured_at);
        thread.fingerprint = manifest.hash;
        let original_exists = paths::guard_path(Path::new(&thread.path), false).is_ok()
            && Path::new(&thread.path).is_file();
        thread.recoverability = if original_exists {
            "source-present"
        } else if thread.integrity == "valid" {
            "recoverable"
        } else {
            "unsupported"
        }
        .into();
        if !original_exists {
            thread.integrity = "missing".into();
        }
        if let Some(position) = positions.get(&thread.key) {
            index.threads[*position] = thread;
        } else {
            index.threads.push(thread);
        }
    }
    index.protection.protected_count = index
        .threads
        .iter()
        .filter(|thread| thread.snapshot == SnapshotState::Protected)
        .count() as u64;
    index.protection.bytes_protected = index
        .threads
        .iter()
        .filter_map(|thread| thread.protected_bytes)
        .sum();
    index.protection.error = Some(if warnings > 0 {
        format!("已找回 {recovered_count} 项待提交副本；另有 {warnings} 项保护数据无法校验，已保留证据，其他线程不受影响。")
    } else {
        "已找回中断前完成保护的线程副本，请重新扫描确认来源。".into()
    });
    write(paths, &index)
}

pub(in crate::threads) fn rebuild(paths: &AppPaths) -> Result<ThreadIndex, String> {
    let _lock = lock(paths)?;
    let root = index_directory(paths);
    let index = from_manifests(paths)?;
    let token = uuid::Uuid::new_v4().to_string();
    let staging = root.join(format!("inventory-rebuild-{token}.sqlite3"));
    {
        let mut connection = open_at(&staging, true)?;
        write_connection(&mut connection, &index)?;
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
            .map_err(|_| "线程索引重建检查点未完成。")?;
        let result: String = connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(|_| "无法验证重建后的线程索引。")?;
        if result != "ok" {
            return Err("重建后的线程索引未通过验证，原文件已保留。".into());
        }
    }
    let database = root.join("inventory.sqlite3");
    if database.exists() {
        paths::guard_path(&database, false)?;
        fs::rename(
            &database,
            root.join(format!("inventory-preserved-{token}.sqlite3")),
        )
        .map_err(|_| "原线程索引无法保留，未替换索引。")?;
        for suffix in ["-wal", "-shm"] {
            let path = root.join(format!("inventory.sqlite3{suffix}"));
            if path.exists() {
                paths::guard_path(&path, false)?;
                fs::rename(
                    path,
                    root.join(format!("inventory-preserved-{token}.sqlite3{suffix}")),
                )
                .map_err(|_| "原索引日志无法保留，请保留数据后重试。")?;
            }
        }
    }
    vault::publish_file(&staging, &database)?;
    Ok(index)
}
