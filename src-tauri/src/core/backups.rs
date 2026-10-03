//! Encrypted snapshots, guarded commits, and byte-exact restoration.
use super::{
    changes::{changes, describe, digest, raw, token},
    configuration::{active_profile_id, profile_matches_configuration},
    domain::{now, validate_id, Backup, Change, ChangePreview},
    filesystem::{atomic_write, config_text, read_config},
    paths::AppPaths,
    store::{load_store, save_store},
};
use std::fs;
use toml_edit::DocumentMut;
use uuid::Uuid;
use zeroize::Zeroizing;

pub(crate) fn commit_config(
    paths: &AppPaths,
    current: Option<Zeroizing<Vec<u8>>>,
    proposed: Option<&[u8]>,
    reason: &str,
    summary: &str,
) -> Result<Backup, String> {
    let mut store = load_store(paths)?;
    let backup = Backup {
        id: Uuid::new_v4().to_string(),
        created_at: now(),
        reason: reason.into(),
        summary: summary.into(),
        config_existed: current.is_some(),
        config_path: paths.config.to_string_lossy().into(),
    };
    let encrypted = crate::security::protect(raw(&current).unwrap_or_default())?;
    let backup_path = paths.backup_file(&backup.id)?;
    atomic_write(&backup_path, &encrypted)?;
    store.backups.insert(0, backup.clone());
    save_store(paths, &store)?;
    let latest = read_config(paths)?;
    if digest(raw(&latest)) != digest(raw(&current)) {
        return Err("配置正在被其他程序修改。已保留备份，请重新预览。".into());
    }
    match proposed {
        Some(bytes) => atomic_write(&paths.config, bytes)?,
        None => match fs::remove_file(&paths.config) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err("无法移除配置文件，原文件保持不变。".into()),
        },
    }
    // Read-back checks detect unexpected filesystem failures without exposing configuration content.
    let written = read_config(paths)?;
    if digest(raw(&written)) != digest(proposed) {
        return Err(
            "配置写入后与预期不同，可能正被其他程序修改。请在恢复页面使用刚创建的备份。".into(),
        );
    }
    Ok(backup)
}
pub fn load_backup(
    paths: &AppPaths,
    id: &str,
) -> Result<(Backup, Option<Zeroizing<Vec<u8>>>), String> {
    validate_id(id)?;
    let backup = load_store(paths)?
        .backups
        .into_iter()
        .find(|b| b.id == id)
        .ok_or("备份不存在。")?;
    if backup.config_path != paths.config.to_string_lossy() {
        return Err("此备份属于另一个 CODEX_HOME，不能恢复到当前配置目录。".into());
    }
    let bytes = fs::read(paths.backup_file(id)?).map_err(|_| "无法读取备份文件。")?;
    let content = Zeroizing::new(crate::security::unprotect(&bytes)?);
    if backup.config_existed {
        let text = std::str::from_utf8(&content).map_err(|_| "此备份不是有效的 UTF-8 配置。")?;
        text.parse::<DocumentMut>()
            .map_err(|_| "此备份本身存在 TOML 语法错误，不能作为自动恢复目标。")?;
        Ok((backup, Some(content)))
    } else {
        Ok((backup, None))
    }
}
pub fn preview_restore(paths: &AppPaths, id: &str) -> Result<ChangePreview, String> {
    let (backup, proposed) = load_backup(paths, id)?;
    validate_restore_target(paths, &proposed)?;
    let current = read_config(paths)?;
    let before = config_text(&current).unwrap_or("<invalid config>");
    let after = config_text(&proposed)?;
    let mut delta = changes(before, after);
    delta.push(Change {
        label: "恢复范围".into(),
        before: "当前完整配置".into(),
        after: if backup.config_existed {
            "恢复备份中的完整配置（包括其他设置）".into()
        } else {
            "移除配置文件，恢复创建前状态".into()
        },
    });
    Ok(ChangePreview { id: Uuid::new_v4().to_string(), title: "恢复配置备份".into(), summary: "恢复前会备份当前状态，便于撤销本次操作。已保存的登录信息不会改变。请关闭正在运行的 Codex 后恢复，再重新打开。".into(), changes: delta, expected_hash: token(raw(&current), raw(&proposed)), profile_id: None, backup_id: Some(id.into()) })
}
pub fn restore_backup(paths: &AppPaths, id: &str, expected_hash: &str) -> Result<Backup, String> {
    let _lock = paths.lock()?;
    let (_, proposed) = load_backup(paths, id)?;
    let current = read_config(paths)?;
    if token(raw(&current), raw(&proposed)) != expected_hash {
        return Err("配置在预览后发生变化，请重新预览恢复内容。".into());
    }
    validate_restore_target(paths, &proposed)?;
    let restored =
        crate::catalog::restored_store(paths, config_text(&proposed)?, &load_store(paths)?)?;
    let backup = commit_config(
        paths,
        current,
        raw(&proposed),
        "恢复备份",
        "恢复之前保存的完整配置",
    )?;
    if let Some(mut store) = restored {
        store.backups = load_store(paths)?.backups;
        save_store(paths, &store)?;
    }
    Ok(backup)
}
fn validate_restore_target(
    paths: &AppPaths,
    proposed: &Option<Zeroizing<Vec<u8>>>,
) -> Result<(), String> {
    let text = config_text(proposed)?;
    if crate::catalog::restored_store(paths, text, &load_store(paths)?)?.is_some() {
        return Ok(());
    }
    if !describe(text).0.starts_with("vela_") {
        return Ok(());
    }
    let store = load_store(paths)?;
    let id = active_profile_id(text, &store.profiles)
        .ok_or("此备份使用的受管连接已被删除。请重新创建连接并应用，或选择其他备份。")?;
    let profile = store
        .profiles
        .iter()
        .find(|profile| profile.id == id)
        .ok_or("备份引用的连接不存在。")?;
    // Backup snapshots do not include historical credential versions. In particular,
    // never send a newly rotated key to a historical endpoint after restoring a file.
    if !paths.helper.is_file() || !profile_matches_configuration(text, profile, &paths.helper) {
        return Err("备份中的连接地址、模型或凭据助手已与保存记录不同。请重新应用所需连接，或选择其他备份。".into());
    }
    let _key = Zeroizing::new(crate::security::get_secret(&id)?);
    Ok(())
}
