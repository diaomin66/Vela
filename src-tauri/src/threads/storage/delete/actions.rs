use super::super::{inventory, paths::hash, restore, vault};
use super::{
    journal::{self, Record},
    plan,
};
use crate::{
    core::AppPaths,
    threads::{client, reconcile, types::*},
};
use chrono::Utc;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

fn start(paths: &AppPaths, source: &ThreadSource) -> Result<client::Client, String> {
    if let Some(error) = paths.location_error() {
        return Err(error.to_owned());
    }
    let executable = client::discover()?;
    let db_root = plan::database(paths, source)?;
    let mut client = client::Client::start(&executable, Path::new(&source.root), Some(&db_root))?;
    client.verify_mutation_scope()?;
    Ok(client)
}

pub(in crate::threads) fn delete(
    paths: &AppPaths,
    keys: Vec<String>,
    expected_hash: String,
) -> Result<ThreadDeletionResult, String> {
    let _lock = paths.lock()?;
    if let Some(error) = paths.location_error() {
        return Err(error.to_owned());
    }
    let index = inventory::read(paths)?;
    let mut groups = plan::groups(&index, &keys)?;
    if plan::token(&index, &groups)? != expected_hash {
        return Err("线程清单在预览后已变化，请重新预览删除。".into());
    }
    let preview = plan::preview_delete(paths, keys)?;
    let mut result = ThreadDeletionResult {
        items: Vec::new(),
        deleted_count: 0,
        failed_count: 0,
    };
    let mut allowed = HashSet::new();
    for item in preview.items {
        if item.can_delete {
            allowed.insert(item.key);
        } else {
            result.failed_count += 1;
            result.items.push(ThreadDeletionResultItem {
                key: item.key,
                thread_id: item.thread_id,
                status: "blocked".into(),
                trash_id: None,
                message: item
                    .reason
                    .unwrap_or_else(|| "此线程暂不能安全删除。".into()),
            });
        }
    }
    groups.retain(|group| allowed.contains(&group[0].key));
    let mut readers = HashMap::new();
    let mut clients = HashMap::new();
    // Complete every preflight and backup before publishing any deletion.
    for group in &groups {
        let first = &group[0];
        let source = plan::source(&index, first)?;
        readers.insert(
            first.key.clone(),
            plan::protect_sources(paths, source, group)?,
        );
        if !clients.contains_key(&source.id) {
            reconcile::backup_metadata(paths, source, &plan::database(paths, source)?)?;
            clients.insert(source.id.clone(), start(paths, source)?);
        }
        plan::verify_scope(source, group)?;
        plan::verify_graph(paths, source, &first.thread_id)?;
    }
    let mut store = journal::read(paths)?;
    for (position, group) in groups.iter().enumerate() {
        let first = &group[0];
        let source = plan::source(&index, first)?;
        let id = uuid::Uuid::new_v4().to_string();
        let mut record = Record {
            version: 1,
            id: id.clone(),
            deleted_at: Utc::now().to_rfc3339(),
            state: "prepared".into(),
            message: None,
            items: group.clone(),
        };
        // The encrypted journal is also the logical tombstone; one atomic
        // publication makes a crash between RPC and inventory update safe.
        if let Err(error) = journal::update(paths, &mut store, &record) {
            not_executed(
                &mut result,
                &groups[position..],
                &format!("删除事务未能保存，此项未发出删除请求：{error}"),
            );
            break;
        }
        let rpc = clients.get_mut(&source.id).ok_or("本机线程服务不可用。")?;
        let outcome = (|| {
            plan::verify_scope(source, &group)?;
            plan::verify_graph(paths, source, &first.thread_id)?;
            rpc.delete_thread(&first.thread_id)
        })();
        drop(readers.remove(&first.key));
        let outcome = outcome.and_then(|_| verify_deleted(rpc, &group));
        let (status, message) = match outcome {
            Ok(()) => {
                result.deleted_count += 1;
                record.state = "deleted".into();
                (
                    "deleted",
                    "线程已移入回收站，可撤销恢复会话记录。".to_owned(),
                )
            }
            Err(error) => {
                // Even a timeout can follow a partial filesystem/database
                // deletion. Never release this tombstone based on an RPC error.
                result.failed_count += 1;
                record.state = "interrupted".into();
                let message =
                    format!("删除未完全确认：{error} 加密副本已保留，可在回收站预览撤销。");
                record.message = Some(message.clone());
                ("interrupted", message)
            }
        };
        // A failed final journal write leaves the durable prepared tombstone.
        // Report completed deletion accurately and stop before further work.
        let persisted = journal::update(paths, &mut store, &record);
        let message = match &persisted {
            Ok(()) => message,
            Err(error) => format!("{message} 回收站最终状态未保存：{error}"),
        };
        result.items.push(ThreadDeletionResultItem {
            key: first.key.clone(),
            thread_id: first.thread_id.clone(),
            status: status.into(),
            trash_id: Some(id),
            message,
        });
        if persisted.is_err() {
            not_executed(
                &mut result,
                &groups[position + 1..],
                "前一项事务未能完整保存，此项尚未执行。",
            );
            break;
        }
    }
    Ok(result)
}

fn verify_deleted(rpc: &mut client::Client, group: &[ThreadSummary]) -> Result<(), String> {
    for thread in group {
        if Path::new(&thread.path)
            .try_exists()
            .map_err(|_| "无法确认源记录删除状态。")?
        {
            return Err("本机服务返回成功，但原始记录仍存在。".into());
        }
    }
    let id = &group[0].thread_id;
    if rpc.list_ids(false)?.contains(id) || rpc.list_ids(true)?.contains(id) {
        return Err("原始记录已移除，但官方索引仍有该线程，请从回收站撤销后检查。".into());
    }
    Ok(())
}

fn restore_previews(
    paths: &AppPaths,
    record: &Record,
) -> Result<Vec<ThreadRestorePreview>, String> {
    let mut previews = Vec::new();
    for thread in &record.items {
        let manifest = vault::selected(paths, thread)?.ok_or("回收站保护副本缺失。")?;
        if manifest.hash != thread.fingerprint || manifest.bytes != thread.bytes {
            return Err("回收站的精确保护副本不可用，未改用其它版本。".into());
        }
        vault::verify(paths, &manifest)?;
        let preview = restore::preview_restore(paths, thread)?;
        if preview.conflict {
            return Err("原位置已有不同内容，撤销不会覆盖现有记录。".into());
        }
        previews.push(preview);
    }
    Ok(previews)
}

fn restore_token(record: &Record, previews: &[ThreadRestorePreview]) -> Result<String, String> {
    let proofs: Vec<_> = previews
        .iter()
        .map(|preview| (&preview.thread.key, &preview.expected_hash))
        .collect();
    Ok(hash(
        &serde_json::to_vec(&(&record.id, &record.items, proofs))
            .map_err(|_| "无法生成回收站恢复预览。")?,
    ))
}

pub(in crate::threads) fn preview_trash_restore(
    paths: &AppPaths,
    id: String,
) -> Result<ThreadTrashRestorePreview, String> {
    let (_, record) = journal::find(paths, &id)?;
    let previews = restore_previews(paths, &record);
    let (expected_hash, reason) = match previews {
        Ok(previews) => (restore_token(&record, &previews)?, None),
        Err(error) => (String::new(), Some(error)),
    };
    Ok(ThreadTrashRestorePreview { id, thread_id: record.items[0].thread_id.clone(), rollout_count: record.items.len() as u64,
        expected_hash, can_restore: reason.is_none(), reason,
        warning: "恢复全部原始会话记录并尝试刷新官方索引；附件、目标等独立元数据不保证还原。已有不同内容时停止，保留回收站副本。".into() })
}

pub(in crate::threads) fn restore_trash(
    paths: &AppPaths,
    id: String,
    expected_hash: String,
) -> Result<ThreadDeletionResult, String> {
    restore_with_refresh(paths, id, expected_hash, refresh)
}

fn not_executed(result: &mut ThreadDeletionResult, groups: &[Vec<ThreadSummary>], message: &str) {
    for group in groups {
        result.failed_count += 1;
        result.items.push(ThreadDeletionResultItem {
            key: group[0].key.clone(),
            thread_id: group[0].thread_id.clone(),
            status: "blocked".into(),
            trash_id: None,
            message: message.into(),
        });
    }
}

pub(super) fn restore_with_refresh(
    paths: &AppPaths,
    id: String,
    expected_hash: String,
    refresh: impl FnOnce(&AppPaths, &Record) -> Result<(), String>,
) -> Result<ThreadDeletionResult, String> {
    let _lock = paths.lock()?;
    if let Some(error) = paths.location_error() {
        return Err(error.to_owned());
    }
    let (mut store, mut record) = journal::find(paths, &id)?;
    let previews = restore_previews(paths, &record)?;
    if restore_token(&record, &previews)? != expected_hash {
        return Err("副本、依赖或目标在预览后变化，请重新预览撤销。".into());
    }
    record.state = "restoring".into();
    journal::update(paths, &mut store, &record)?;
    let mut result = ThreadDeletionResult {
        items: Vec::new(),
        deleted_count: 0,
        failed_count: 0,
    };
    // Use the exact previews that were validated against the user's token;
    // recomputing a fresh per-file token here would silently accept a race.
    for preview in previews {
        let outcome = restore::apply_restore(paths, &preview.thread, &preview.expected_hash);
        let (status, message) = match outcome {
            Ok(()) => {
                result.deleted_count += 1;
                ("restored", "会话记录已恢复并通过校验。".into())
            }
            Err(error) => {
                result.failed_count += 1;
                ("failed", error)
            }
        };
        result.items.push(ThreadDeletionResultItem {
            key: preview.thread.key,
            thread_id: preview.thread.thread_id,
            status: status.into(),
            trash_id: Some(id.clone()),
            message,
        });
    }
    if result.failed_count == 0 {
        // Release tombstone only after every original byte stream is verified.
        record.state = "restored".into();
        record.message = None;
    } else {
        record.state = "interrupted".into();
        record.message = Some("部分记录尚未恢复，完整副本仍保留，可重新预览重试。".into());
    }
    if let Err(error) = journal::update(paths, &mut store, &record) {
        result.deleted_count = 0;
        result.failed_count = result.items.len() as u64;
        for item in &mut result.items {
            item.status = "interrupted".into();
            item.message
                .push_str(&format!(" 回收站状态尚未更新，需重新预览完成撤销：{error}"));
        }
        return Ok(result);
    }
    if result.failed_count == 0 {
        if let Err(error) = refresh(paths, &record) {
            for item in &mut result.items {
                item.message
                    .push_str(&format!(" 官方列表尚未刷新：{error}"));
            }
        }
    }
    Ok(result)
}

fn refresh(paths: &AppPaths, record: &Record) -> Result<(), String> {
    let index = inventory::read(paths)?;
    let source = plan::source(&index, &record.items[0])?;
    let mut rpc = start(paths, source)?;
    rpc.list_all(false)?;
    rpc.list_all(true)?;
    Ok(())
}
