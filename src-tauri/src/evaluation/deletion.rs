use super::{storage, types::RunStatus};
use crate::core::{self, AppPaths};
use std::{collections::HashSet, fs, io::ErrorKind};

pub(super) fn remove(paths: &AppPaths, ids: &[String]) -> Result<Option<String>, String> {
    let _lock = paths.lock()?;
    let mut store = storage::read(paths)?;
    let selected: HashSet<&str> = ids.iter().map(String::as_str).collect();
    if selected.is_empty() || selected.len() > storage::HISTORY_LIMIT {
        return Err("请选择 1–100 次评测记录。".into());
    }
    for id in &selected {
        core::validate_id(id).map_err(|_| "评测记录标识无效。")?;
        if store.active_run_id.as_deref() == Some(*id) {
            return Err("正在运行的评测不能删除，请先取消并等待结束。".into());
        }
        let summary = store
            .history
            .iter()
            .find(|summary| summary.id == *id)
            .ok_or("部分评测记录已不存在，请刷新后重新选择；本次未删除任何记录。")?;
        if summary.status == RunStatus::Running {
            return Err("正在运行的评测不能删除，请先取消并等待结束。".into());
        }
        match fs::symlink_metadata(storage::run_path(paths, id)?) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => return Err("评测记录路径异常，本次未删除任何记录。".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err("无法访问评测记录，本次未删除任何记录。".into()),
        }
    }
    recover_staged(paths, &store)?;
    let staging = storage::directory(paths).join(".deleting");
    fs::create_dir_all(&staging).map_err(|_| "无法暂存待删除评测，本次未删除任何记录。")?;
    for id in &selected {
        let source = storage::run_path(paths, id)?;
        match fs::rename(&source, staging.join(format!("{id}.json"))) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => {
                return match recover_staged(paths, &store) {
                    Ok(()) => Err("评测文件正在使用或无法修改，本次未删除任何记录。".into()),
                    Err(error) => Err(error),
                };
            }
        }
    }
    store
        .history
        .retain(|run| !selected.contains(run.id.as_str()));
    store
        .records
        .retain(|record| !selected.contains(record.run_id.as_str()));
    if let Err(error) = storage::write(paths, &store) {
        recover_staged(paths, &storage::read(paths)?)?;
        return Err(error);
    }
    Ok(recover_staged(paths, &store)
        .err()
        .map(|_| "评测记录已删除；部分暂存文件仍被占用，重新打开软件后会继续清理。".into()))
}

pub(super) fn recover(paths: &AppPaths) -> Result<(), String> {
    let _lock = paths.lock()?;
    recover_staged(paths, &storage::read(paths)?)
}

fn recover_staged(paths: &AppPaths, store: &storage::EvaluationStore) -> Result<(), String> {
    let staging = storage::directory(paths).join(".deleting");
    let entries = match fs::read_dir(&staging) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("无法读取评测删除暂存目录，请检查访问权限。".into()),
    };
    for entry in entries {
        let entry = entry.map_err(|_| "无法读取评测删除暂存记录。")?;
        let path = entry.path();
        let Some(id) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().and_then(|extension| extension.to_str()) != Some("json")
            || core::validate_id(id).is_err()
        {
            continue;
        }
        if !entry
            .file_type()
            .map_err(|_| "无法检查评测暂存记录。")?
            .is_file()
        {
            return Err("评测删除暂存路径异常，请保留文件后检查。".into());
        }
        if store.history.iter().any(|run| run.id == id)
            || store.active_run_id.as_deref() == Some(id)
        {
            let destination = storage::run_path(paths, id)?;
            if destination.exists() {
                return Err("评测记录与删除暂存文件冲突，请保留文件后检查。".into());
            }
            fs::rename(&path, destination)
                .map_err(|_| "评测删除尚未提交，恢复原记录失败；重新打开软件后将重试。")?;
        } else {
            fs::remove_file(&path).map_err(|_| "无法清理已删除评测的暂存文件。")?;
        }
    }
    let _ = fs::remove_dir(staging);
    Ok(())
}
