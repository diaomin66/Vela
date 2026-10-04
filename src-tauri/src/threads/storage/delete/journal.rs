use super::super::paths::{self, directory};
use crate::{
    core::{self, AppPaths},
    security,
    threads::types::*,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs};
use zeroize::Zeroizing;

const MAX_STORE: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Record {
    pub version: u32,
    pub id: String,
    pub deleted_at: String,
    pub state: String,
    pub message: Option<String>,
    pub items: Vec<ThreadSummary>,
}

impl Record {
    pub fn hidden(&self) -> bool {
        matches!(
            self.state.as_str(),
            "prepared" | "deleted" | "interrupted" | "restoring"
        )
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Store {
    version: u32,
    pub records: Vec<Record>,
}

pub(super) fn read(paths: &AppPaths) -> Result<Store, String> {
    let path = directory(paths).join("trash.bin");
    if !path.try_exists().map_err(|_| "无法检查线程回收站。")? {
        return Ok(Store {
            version: 1,
            records: Vec::new(),
        });
    }
    paths::guard_path(&path, false)?;
    let metadata = fs::metadata(&path).map_err(|_| "无法读取线程回收站。")?;
    if !metadata.is_file() || metadata.len() > MAX_STORE {
        return Err("线程回收站超过安全读取范围，已停止扫描以保留删除状态。".into());
    }
    let encrypted = fs::read(path).map_err(|_| "无法读取线程回收站。")?;
    let raw = Zeroizing::new(security::unprotect(&encrypted)?);
    if raw.len() as u64 > MAX_STORE {
        return Err("线程回收站数据过大。".into());
    }
    let store: Store = serde_json::from_slice(&raw)
        .map_err(|_| "线程回收站记录损坏，已停止扫描以避免恢复已删除线程。")?;
    if store.version != 1 || store.records.len() > 20_000 {
        return Err("线程回收站版本或数量不受支持。".into());
    }
    let mut ids = HashSet::new();
    for record in &store.records {
        if record.version != 1
            || uuid::Uuid::parse_str(&record.id).is_err()
            || !ids.insert(&record.id)
            || record.items.is_empty()
            || record.items.len() > 200_000
            || !matches!(
                record.state.as_str(),
                "prepared" | "deleted" | "interrupted" | "restoring" | "restored" | "failed"
            )
        {
            return Err("线程回收站事务无效，已保留副本并停止更新清单。".into());
        }
        let first = &record.items[0];
        for item in &record.items {
            paths::safe_id(&item.source_id)?;
            paths::safe_hash(&item.fingerprint)?;
            paths::relative(&item.relative_path)?;
            if item.source_id != first.source_id
                || item.thread_id != first.thread_id
                || uuid::Uuid::parse_str(&item.thread_id).is_err()
            {
                return Err("线程回收站逻辑标识不一致。".into());
            }
        }
    }
    Ok(store)
}

pub(super) fn write(paths: &AppPaths, store: &Store) -> Result<(), String> {
    paths::ensure_directory(&directory(paths))?;
    let raw = Zeroizing::new(serde_json::to_vec(store).map_err(|_| "无法编码线程回收站。")?);
    if raw.len() as u64 > MAX_STORE {
        return Err("线程回收站已满，未开始新的删除。".into());
    }
    let encrypted = security::protect(&raw)?;
    core::atomic_write(&directory(paths).join("trash.bin"), &encrypted)
}

pub(super) fn update(paths: &AppPaths, store: &mut Store, record: &Record) -> Result<(), String> {
    store.records.retain(|old| old.id != record.id);
    store.records.push(record.clone());
    write(paths, store)
}

pub(super) fn find(paths: &AppPaths, id: &str) -> Result<(Store, Record), String> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err("回收站标识无效。".into());
    }
    let store = read(paths)?;
    let record = store
        .records
        .iter()
        .find(|record| record.id == id && record.hidden())
        .cloned()
        .ok_or("回收站记录不存在或已撤销。")?;
    Ok((store, record))
}

/// One load per scan/rebuild. Read or decryption failures must stop the caller.
pub(in crate::threads) fn tombstones(
    paths: &AppPaths,
) -> Result<HashSet<(String, String)>, String> {
    Ok(read(paths)?
        .records
        .into_iter()
        .filter(Record::hidden)
        .map(|record| {
            let first = &record.items[0];
            (first.source_id.clone(), first.thread_id.clone())
        })
        .collect())
}

pub(in crate::threads) fn audit(paths: &AppPaths) -> Result<(), String> {
    let mut store = read(paths)?;
    let mut changed = false;
    for record in &mut store.records {
        if matches!(record.state.as_str(), "prepared" | "restoring") {
            record.state = "interrupted".into();
            record.message =
                Some("上次操作中断；加密副本与删除标记已保留，可重新预览撤销。".into());
            changed = true;
        }
    }
    if changed {
        write(paths, &store)?;
    }
    Ok(())
}

pub(in crate::threads) fn list_trash(
    paths: &AppPaths,
    offset: u32,
    limit: u32,
) -> Result<ThreadTrashPage, String> {
    if limit == 0 || limit > 100 {
        return Err("回收站每页显示数量需为 1–100 条。".into());
    }
    let mut records: Vec<_> = read(paths)?
        .records
        .into_iter()
        .filter(Record::hidden)
        .collect();
    records.sort_by(|left, right| right.deleted_at.cmp(&left.deleted_at));
    let total = records.len() as u64;
    let items = records
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|record| {
            let first = &record.items[0];
            ThreadTrashItem {
                id: record.id,
                thread_id: first.thread_id.clone(),
                source_id: first.source_id.clone(),
                title: first.title.clone(),
                deleted_at: record.deleted_at,
                rollout_count: record.items.len() as u64,
                bytes: record.items.iter().map(|item| item.bytes).sum(),
                state: record.state,
                message: record.message,
            }
        })
        .collect();
    Ok(ThreadTrashPage {
        items,
        total,
        offset,
        limit,
    })
}
