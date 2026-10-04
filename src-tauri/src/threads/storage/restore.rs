use super::{
    dependencies, inventory,
    paths::{self, directory, hash},
    vault,
};
use crate::{
    core::{self, AppPaths},
    security,
    threads::types::{ThreadRestorePreview, ThreadSummary},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RestoreJournal {
    version: u32,
    id: String,
    created_at: String,
    key: String,
    source_id: String,
    snapshot_hash: String,
    target: String,
    staging: String,
    state: String,
}

fn registered_target(paths: &AppPaths, manifest: &vault::Manifest) -> Result<PathBuf, String> {
    let index = inventory::read(paths)?;
    let source = index
        .sources
        .iter()
        .find(|source| source.id == manifest.thread.source_id)
        .ok_or("线程来源不在当前保护范围，请先添加来源并重新扫描。")?;
    let root = Path::new(&source.root);
    if paths::normalized(root)? != paths::normalized(Path::new(&manifest.source_root))? {
        return Err("线程快照属于其他目录，不能恢复到当前来源。".into());
    }
    paths::guard_path(root, false)?;
    if !fs::metadata(root)
        .map_err(|_| "线程来源暂时不可用。")?
        .is_dir()
    {
        return Err("线程来源不是有效目录。".into());
    }
    let relative = paths::relative(&manifest.thread.relative_path)?;
    let target = root.join(relative);
    paths::guard_path(&target, true)?;
    Ok(target)
}

fn restore_manifest(paths: &AppPaths, thread: &ThreadSummary) -> Result<vault::Manifest, String> {
    let manifest = vault::selected(paths, thread)?.ok_or("此线程没有可验证的保护副本。")?;
    if manifest.thread.integrity != "valid" {
        return Err("该副本包含未知或不完整的记录格式，已保留原始内容，暂不自动恢复。".into());
    }
    Ok(manifest)
}

fn token(
    thread: &ThreadSummary,
    manifest: &vault::Manifest,
    target: &Path,
    target_hash: &Option<String>,
    dependencies: &[dependencies::Proof],
) -> Result<String, String> {
    let payload = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "key": thread.key,
        "sourceId": thread.source_id,
        "threadId": thread.thread_id,
        "snapshotHash": manifest.hash,
        "snapshotBytes": manifest.bytes,
        "target": paths::normalized(target)?,
        "targetHash": target_hash,
        "dependencies": dependencies,
    }))
    .map_err(|_| "无法生成线程恢复预览。")?;
    Ok(hash(&payload))
}

pub(in crate::threads) fn preview_restore(
    paths: &AppPaths,
    thread: &ThreadSummary,
) -> Result<ThreadRestorePreview, String> {
    let manifest = restore_manifest(paths, thread)?;
    let target = registered_target(paths, &manifest)?;
    let target_hash = vault::file_hash(&target)?;
    let dependencies = dependencies::validate(paths, &manifest)?;
    let conflict = target_hash
        .as_ref()
        .is_some_and(|existing| existing != &manifest.hash);
    let expected_hash = token(
        thread,
        &manifest,
        &target,
        &target_hash,
        &dependencies.proofs,
    )?;
    let mut warning = if conflict {
        Some("原位置已有不同内容，自动恢复不会覆盖它。请保留现有记录后检查。".into())
    } else if target_hash.is_some() {
        Some("原始记录已完整存在，无需重新写入；若列表不可见，请执行索引检查。".into())
    } else {
        Some(
            "将把已验证的完整记录恢复到原位置。侧栏是否显示仍取决于官方索引、归档与筛选状态。"
                .into(),
        )
    };
    if let Some(snapshot_warning) = &manifest.warning {
        warning = Some(format!(
            "{snapshot_warning} {}",
            warning.unwrap_or_default()
        ));
    }
    Ok(ThreadRestorePreview {
        thread: thread.clone(),
        snapshot_hash: manifest.hash,
        target_path: target.to_string_lossy().into_owned(),
        target_exists: target_hash.is_some(),
        target_hash,
        conflict,
        expected_hash,
        warning,
    })
}

fn write_journal(paths: &AppPaths, journal: &RestoreJournal) -> Result<(), String> {
    let folder = directory(paths).join("restores");
    paths::ensure_directory(&folder)?;
    let target = folder.join(format!("{}.bin", journal.id));
    paths::guard_path(&target, true)?;
    let raw = Zeroizing::new(serde_json::to_vec(journal).map_err(|_| "无法记录线程恢复事务。")?);
    core::atomic_write(&target, &security::protect(&raw)?)
}

pub(in crate::threads) fn apply_restore(
    paths: &AppPaths,
    thread: &ThreadSummary,
    expected_hash: &str,
) -> Result<(), String> {
    perform_restore(paths, thread, expected_hash, &mut |journal| {
        write_journal(paths, journal)
    })
}

fn perform_restore(
    paths: &AppPaths,
    thread: &ThreadSummary,
    expected_hash: &str,
    record: &mut impl FnMut(&RestoreJournal) -> Result<(), String>,
) -> Result<(), String> {
    let manifest = restore_manifest(paths, thread)?;
    let target = registered_target(paths, &manifest)?;
    let target_hash = vault::file_hash(&target)?;
    let dependencies = dependencies::validate(paths, &manifest)?;
    if token(
        thread,
        &manifest,
        &target,
        &target_hash,
        &dependencies.proofs,
    )? != expected_hash
    {
        return Err("线程或目标文件在预览后发生变化，请重新预览。".into());
    }
    if let Some(existing) = target_hash {
        return if existing == manifest.hash {
            Ok(())
        } else {
            Err("目标已有不同内容，未覆盖任何线程记录。".into())
        };
    }
    let parent = target.parent().ok_or("线程恢复目标路径无效。")?;
    paths::ensure_directory(parent)?;
    let id = uuid::Uuid::new_v4().to_string();
    let staging = parent.join(format!(".ahax-restore-{id}.tmp"));
    let mut journal = RestoreJournal {
        version: 1,
        id,
        created_at: chrono::Utc::now().to_rfc3339(),
        key: thread.key.clone(),
        source_id: thread.source_id.clone(),
        snapshot_hash: manifest.hash.clone(),
        target: target.to_string_lossy().into_owned(),
        staging: staging.to_string_lossy().into_owned(),
        state: "prepared".into(),
    };
    record(&journal)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(|_| "无法暂存线程恢复内容。")?;
        vault::stream(paths, &manifest, &mut file)?;
        file.flush()
            .and_then(|_| file.sync_all())
            .map_err(|_| "线程恢复内容未能完整落盘。")?;
        drop(file);
        paths::guard_path(parent, false)?;
        if vault::file_hash(&target)?.is_some() {
            return Err("目标在恢复期间被其他程序创建，未覆盖该记录。".into());
        }
        vault::publish_file(&staging, &target)?;
        journal.state = "published".into();
        let _ = record(&journal);
        if vault::file_hash(&target)?.as_deref() != Some(manifest.hash.as_str()) {
            return Err("恢复后内容发生变化；保护副本仍保留，请重新扫描。".into());
        }
        journal.state = "verified".into();
        let _ = record(&journal);
        Ok(())
    })();
    if result.is_err() && staging.exists() {
        if vault::file_hash(&staging).ok().flatten().as_deref() == Some(manifest.hash.as_str()) {
            let _ = fs::remove_file(&staging);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_restore_stays_successful_when_final_journal_writes_fail() {
        let sandbox = tempfile::tempdir().unwrap();
        let (paths, source, thread, bytes) = super::super::tests::fixture(sandbox.path());
        super::super::tests::save(&paths, source, thread.clone());
        vault::protect(&paths, &thread, &bytes).unwrap();
        fs::remove_file(&thread.path).unwrap();
        let preview = preview_restore(&paths, &thread).unwrap();
        let mut calls = 0;
        perform_restore(&paths, &thread, &preview.expected_hash, &mut |journal| {
            calls += 1;
            if calls == 1 {
                write_journal(&paths, journal)
            } else {
                Err("injected journal write failure".into())
            }
        })
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(fs::read(&thread.path).unwrap(), bytes);
        audit(&paths).unwrap();
        let path = fs::read_dir(directory(&paths).join("restores"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let raw = security::unprotect(&fs::read(path).unwrap()).unwrap();
        assert_eq!(
            serde_json::from_slice::<RestoreJournal>(&raw)
                .unwrap()
                .state,
            "verified"
        );
    }
}

pub(super) fn audit(paths: &AppPaths) -> Result<(), String> {
    let folder = directory(paths).join("restores");
    if !folder.exists() {
        return Ok(());
    }
    paths::guard_path(&folder, false)?;
    let mut interrupted = false;
    for entry in fs::read_dir(&folder).map_err(|_| "无法检查线程恢复事务。")? {
        let entry = entry.map_err(|_| "无法枚举线程恢复事务。")?;
        if entry.path().extension().and_then(|value| value.to_str()) != Some("bin") {
            continue;
        }
        paths::guard_path(&entry.path(), false)?;
        let metadata = fs::metadata(entry.path()).map_err(|_| "无法读取线程恢复事务。")?;
        if !metadata.is_file() || metadata.len() > 1024 * 1024 {
            return Err("线程恢复事务记录大小异常，原文件已保留。".into());
        }
        let encrypted = fs::read(entry.path()).map_err(|_| "无法读取线程恢复事务。")?;
        let raw = Zeroizing::new(security::unprotect(&encrypted)?);
        let mut journal: RestoreJournal = serde_json::from_slice(&raw)
            .map_err(|_| "线程恢复事务损坏，原文件与保护副本均已保留。")?;
        if journal.version != 1
            || uuid::Uuid::parse_str(&journal.id).is_err()
            || entry.file_name().to_string_lossy() != format!("{}.bin", journal.id)
        {
            return Err("线程恢复事务标识无效，未修改相关文件。".into());
        }
        paths::safe_hash(&journal.snapshot_hash)?;
        if matches!(
            journal.state.as_str(),
            "verified" | "interrupted" | "conflict"
        ) {
            continue;
        }
        if !matches!(journal.state.as_str(), "prepared" | "published") {
            return Err("线程恢复事务状态无法识别，原文件已保留。".into());
        }
        let target = Path::new(&journal.target);
        let staging = Path::new(&journal.staging);
        paths::guard_path(target, true)?;
        paths::guard_path(staging, true)?;
        let parent = target.parent().ok_or("线程恢复事务目标无效。")?;
        let expected_staging = parent.join(format!(".ahax-restore-{}.tmp", journal.id));
        if paths::normalized(staging)? != paths::normalized(&expected_staging)? {
            return Err("线程恢复暂存位置无效，未修改相关文件。".into());
        }
        let index = inventory::read(paths)?;
        let source = index
            .sources
            .iter()
            .find(|source| source.id == journal.source_id)
            .ok_or("上次线程恢复的来源暂不可用，暂存文件已保留。")?;
        let relative = target
            .strip_prefix(&source.root)
            .map_err(|_| "线程恢复事务不属于已登记来源。")?;
        paths::relative(&relative.to_string_lossy())?;
        let current_hash = vault::file_hash(target)?;
        let stage_hash = vault::file_hash(staging)?;
        journal.state = if current_hash.as_deref() == Some(journal.snapshot_hash.as_str()) {
            "verified"
        } else if current_hash.is_some() {
            interrupted = true;
            "conflict"
        } else {
            let _complete_staging = stage_hash.as_deref() == Some(journal.snapshot_hash.as_str());
            interrupted = true;
            "interrupted"
        }
        .into();
        write_journal(paths, &journal)?;
    }
    if interrupted {
        Err("上次线程恢复未完成；已有记录与暂存文件已保留，请重新预览后重试。".into())
    } else {
        Ok(())
    }
}
