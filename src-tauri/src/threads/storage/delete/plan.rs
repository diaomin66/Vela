use super::super::{
    inventory,
    paths::{self, hash},
    vault,
};
use crate::{
    core::AppPaths,
    threads::{reconcile, scan, types::*},
};
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read},
    path::Path,
    time::Duration,
};

pub(super) fn groups(
    index: &ThreadIndex,
    keys: &[String],
) -> Result<Vec<Vec<ThreadSummary>>, String> {
    if keys.is_empty() || keys.len() > 100 || keys.iter().any(|key| key.len() > 256) {
        return Err("单次请选择 1–100 个线程。".into());
    }
    let mut selected = HashSet::new();
    for key in keys {
        let thread = index
            .threads
            .iter()
            .find(|thread| &thread.key == key)
            .ok_or("所选线程已变化，请重新扫描。")?;
        selected.insert((thread.source_id.clone(), thread.thread_id.clone()));
    }
    let mut groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for thread in &index.threads {
        let identity = (thread.source_id.clone(), thread.thread_id.clone());
        if selected.contains(&identity) {
            groups.entry(identity).or_default().push(thread.clone());
        }
    }
    for group in groups.values_mut() {
        group.sort_by(|a, b| a.key.cmp(&b.key));
    }
    Ok(groups.into_values().collect())
}

pub(super) fn source<'a>(
    index: &'a ThreadIndex,
    thread: &ThreadSummary,
) -> Result<&'a ThreadSource, String> {
    index
        .sources
        .iter()
        .find(|source| source.id == thread.source_id)
        .ok_or_else(|| "线程来源不存在。".into())
}

pub(super) fn database(
    paths: &AppPaths,
    source: &ThreadSource,
) -> Result<std::path::PathBuf, String> {
    reconcile::sqlite_root(paths, source)
}

/// The service deletes descendants implicitly. Only leaf threads are supported
/// until the complete subtree can be included in one reviewable transaction.
pub(super) fn verify_graph(
    paths: &AppPaths,
    source: &ThreadSource,
    id: &str,
) -> Result<String, String> {
    let db_root = database(paths, source)?;
    paths::guard_path(&db_root, false)?;
    // A newer service can leave state_5 behind while deleting against a newer
    // database. Never use that stale graph as authority for a destructive RPC.
    for entry in std::fs::read_dir(&db_root).map_err(|_| "无法核对官方数据库版本。")? {
        let name = entry.map_err(|_| "无法核对官方数据库文件。")?.file_name();
        if name
            .to_str()
            .and_then(|name| name.strip_prefix("state_"))
            .and_then(|name| name.strip_suffix(".sqlite"))
            .and_then(|version| version.parse::<u32>().ok())
            .is_some_and(|version| version > 5)
        {
            return Err(
                "发现更新版本的官方线程数据库，当前版本不能安全核对子线程关系，未开始删除。".into(),
            );
        }
    }
    let path = db_root.join("state_5.sqlite");
    paths::guard_path(&path, false).map_err(|_| "无法验证官方线程关联，请先执行索引检查。")?;
    let db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "无法读取官方线程关联，未开始删除。")?;
    db.busy_timeout(Duration::from_millis(250))
        .map_err(|_| "无法设置官方索引等待时间。")?;
    db.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
        .map_err(|_| "无法安全读取官方线程关联。")?;
    let columns: HashSet<String> = db
        .prepare("PRAGMA table_info(thread_spawn_edges)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect()
        })
        .map_err(|_| "官方线程关联格式不受支持。")?;
    if !columns.contains("parent_thread_id") || !columns.contains("child_thread_id") {
        return Err("本机线程索引尚不支持安全删除预检，请更新官方客户端并执行索引检查。".into());
    }
    let mut statement = db.prepare("SELECT child_thread_id FROM thread_spawn_edges WHERE parent_thread_id=?1 ORDER BY child_thread_id LIMIT 2")
        .map_err(|_| "无法读取线程子任务关系。")?;
    let children: Vec<String> = statement
        .query_map([id], |row| row.get(0))
        .and_then(|rows| rows.collect())
        .map_err(|_| "无法读取线程子任务关系。")?;
    if !children.is_empty() {
        return Err("此线程有派生子线程；官方删除会连带移除子线程，请先单独处理子线程。".into());
    }
    Ok(hash(b"verified-leaf-v1"))
}

pub(super) fn dependency_reason(index: &ThreadIndex, group: &[ThreadSummary]) -> Option<String> {
    let first = group.first()?;
    let rollout_ids: HashSet<_> = group
        .iter()
        .filter_map(|thread| scan::filename_rollout_id(Path::new(&thread.path)))
        .collect();
    if index.threads.iter().any(|other| {
        other.source_id == first.source_id
            && other.thread_id != first.thread_id
            && other
                .history_base
                .as_ref()
                .is_some_and(|base| rollout_ids.contains(&base.thread_id))
    }) {
        return Some("此线程的历史仍被其它线程引用，请先处理引用它的线程。".into());
    }
    None
}

fn legacy_snapshot(paths: &AppPaths, manifest: &vault::Manifest) -> Result<(), String> {
    let stream = vault::reader(paths, manifest);
    let stream: Box<dyn Read + '_> = if manifest.thread.relative_path.ends_with(".zst") {
        Box::new(zstd::stream::read::Decoder::new(stream).map_err(|_| "压缩线程副本无法解码。")?)
    } else {
        Box::new(stream)
    };
    let mut reader = BufReader::new(stream.take(8 * 1024 * 1024 + 1));
    let mut bytes = Vec::new();
    loop {
        let count = reader
            .read_until(b'\n', &mut bytes)
            .map_err(|_| "无法核对线程历史格式。")?;
        if count == 0 || bytes.len() > 8 * 1024 * 1024 {
            return Err("线程起始元数据无效，未开始删除。".into());
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "线程起始元数据无效。")?;
        if value.get("type").and_then(serde_json::Value::as_str) != Some("session_meta")
            || value
                .pointer("/payload/id")
                .and_then(serde_json::Value::as_str)
                != Some(manifest.thread.thread_id.as_str())
        {
            return Err("线程副本与逻辑线程标识不一致。".into());
        }
        match value
            .pointer("/payload/history_mode")
            .and_then(serde_json::Value::as_str)
        {
            None | Some("legacy") => return Ok(()),
            Some("paginated") => {
                return Err(
                    "官方只读索引刷新尚不能重建分页消息历史，本版本暂不删除此类线程。".into(),
                )
            }
            Some(_) => return Err("线程使用未知历史格式，暂不允许删除。".into()),
        }
    }
}

/// Deny any writer while still allowing the official service to unlink files.
/// These handles stay alive through RPC completion and its verification.
pub(super) fn protect_sources(
    paths: &AppPaths,
    source: &ThreadSource,
    group: &[ThreadSummary],
) -> Result<Vec<File>, String> {
    let root = Path::new(&source.root);
    paths::guard_path(root, false)?;
    let mut readers = Vec::new();
    for thread in group {
        if thread.snapshot != SnapshotState::Protected || thread.integrity != "valid" {
            return Err("此线程的全部记录尚未完成保护，请先重新扫描。".into());
        }
        let target = root.join(paths::relative(&thread.relative_path)?);
        if paths::normalized(&target)? != paths::normalized(Path::new(&thread.path))? {
            return Err("线程来源路径不一致。".into());
        }
        paths::guard_path(&target, false)?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_DELETE, FILE_SHARE_READ};
            options.share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE);
        }
        readers.push(
            options
                .open(&target)
                .map_err(|_| "线程正在写入或无法读取，请结束正在使用此线程的任务后重试。")?,
        );
        let manifest = vault::selected(paths, thread)?.ok_or("线程没有可验证的保护副本。")?;
        if manifest.hash != thread.fingerprint
            || manifest.bytes != thread.bytes
            || manifest.bytes != manifest.source_bytes
            || manifest.thread.thread_id != thread.thread_id
            || paths::normalized(Path::new(&manifest.source_root))? != paths::normalized(root)?
            || vault::file_hash(&target)?.as_deref() != Some(manifest.hash.as_str())
        {
            return Err("源记录与完整保护副本不一致，请重新扫描后再删除。".into());
        }
        vault::verify(paths, &manifest)?;
        legacy_snapshot(paths, &manifest)?;
    }
    Ok(readers)
}

pub(super) fn verify_scope(source: &ThreadSource, group: &[ThreadSummary]) -> Result<(), String> {
    if !source.available || source.error.is_some() {
        return Err("线程来源未完成扫描，暂不允许删除。".into());
    }
    let id = &group.first().ok_or("线程分组为空。")?.thread_id;
    let mut current = HashSet::new();
    for path in scan::deletion_paths(Path::new(&source.root))? {
        // Unknown file identities could contain another rollout of this ID.
        let found_id =
            scan::filename_id(&path).ok_or("来源包含无法识别的记录，请先处理扫描错误。")?;
        if &found_id == id {
            current.insert(paths::normalized(&path)?);
        }
    }
    let expected: HashSet<_> = group
        .iter()
        .map(|thread| paths::normalized(Path::new(&thread.path)))
        .collect::<Result<_, _>>()?;
    if current != expected {
        return Err("此线程的记录集合已变化，请重新预览删除。".into());
    }
    Ok(())
}

pub(super) fn token(index: &ThreadIndex, groups: &[Vec<ThreadSummary>]) -> Result<String, String> {
    let sources: HashSet<_> = groups
        .iter()
        .map(|group| group[0].source_id.as_str())
        .collect();
    // Bind dependency relationships across the entire source, not only the
    // selected row, while allowing irrelevant verification timestamps to vary.
    let mut rows: Vec<_> = index
        .threads
        .iter()
        .filter(|thread| sources.contains(thread.source_id.as_str()))
        .map(|thread| {
            (
                &thread.key,
                &thread.thread_id,
                &thread.fingerprint,
                &thread.history_base,
            )
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    let selected: Vec<_> = groups.iter().flatten().map(|thread| &thread.key).collect();
    let mut scopes: Vec<_> = index
        .sources
        .iter()
        .filter(|source| sources.contains(source.id.as_str()))
        .map(|source| (&source.id, &source.root, &source.sqlite_home))
        .collect();
    scopes.sort_by(|a, b| a.0.cmp(b.0));
    Ok(hash(
        &serde_json::to_vec(&(selected, rows, scopes)).map_err(|_| "无法生成删除预览。")?,
    ))
}

pub(in crate::threads) fn preview_delete(
    paths: &AppPaths,
    keys: Vec<String>,
) -> Result<ThreadDeletionPreview, String> {
    let index = inventory::read(paths)?;
    let groups = groups(&index, &keys)?;
    let mut items = Vec::new();
    for group in &groups {
        let first = &group[0];
        let checked = (|| {
            if uuid::Uuid::parse_str(&first.thread_id).is_err() {
                return Err("线程标识不受支持。".into());
            }
            if let Some(error) = dependency_reason(&index, group) {
                return Err(error);
            }
            let source = source(&index, first)?;
            verify_scope(source, group)?;
            let _readers = protect_sources(paths, source, group)?;
            verify_graph(paths, source, &first.thread_id)?;
            Ok::<_, String>(())
        })();
        items.push(ThreadDeletionItem {
            key: first.key.clone(),
            thread_id: first.thread_id.clone(),
            source_id: first.source_id.clone(),
            title: first.title.clone(),
            rollout_count: group.len() as u64,
            bytes: group.iter().map(|thread| thread.bytes).sum(),
            can_delete: checked.is_ok(),
            reason: checked.err(),
        });
    }
    Ok(ThreadDeletionPreview { expected_hash: token(&index, &groups)?, logical_count: groups.len() as u64,
        rollout_count: groups.iter().map(Vec::len).sum::<usize>() as u64, items,
        warning: "将删除所选线程的全部会话记录并保留加密副本。撤销恢复原始会话记录；附件、目标等独立元数据不保证还原。".into() })
}
