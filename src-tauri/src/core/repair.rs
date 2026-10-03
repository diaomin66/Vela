//! Pick only a repair target whose channel, credentials, and snapshot remain usable.
use super::{
    backups::{preview_restore, restore_backup},
    configuration::{active_profile_id, apply_profile, preview_profile},
    domain::{Backup, ChangePreview},
    filesystem::{config_text, read_config},
    paths::AppPaths,
    store::load_store,
};
use zeroize::Zeroizing;

pub fn preview_repair(paths: &AppPaths) -> Result<ChangePreview, String> {
    let store = load_store(paths)?;
    let current = read_config(paths)?;
    if let Ok(contents) = config_text(&current) {
        if crate::catalog::is_gateway_config(contents) {
            if let Ok(mut preview) = crate::catalog::preview(paths, None) {
                preview.title = "修复统一模型目录".into();
                return Ok(preview);
            }
        }
        if let Some(id) = active_profile_id(contents, &store.profiles) {
            if paths.helper.is_file()
                && crate::security::get_secret(&id).map(Zeroizing::new).is_ok()
            {
                if let Ok(mut preview) = preview_profile(paths, &id) {
                    preview.title = "修复当前连接配置".into();
                    preview.summary = "重新写入当前连接的模型、地址、Responses 协议和凭据助手设置。其他服务商与无关配置保持原样。".into();
                    return Ok(preview);
                }
            }
        }
    }
    for backup in &store.backups {
        if backup.config_path == paths.config.to_string_lossy() {
            if let Ok(mut preview) = preview_restore(paths, &backup.id) {
                preview.title = "从最近可用备份修复".into();
                return Ok(preview);
            }
        }
    }
    Err("没有可安全自动修复的当前连接或可用备份。请在连接页面选择已保存的连接；配置语法损坏时需先修正具体错误。".into())
}
pub fn apply_repair(paths: &AppPaths, expected_hash: &str) -> Result<Backup, String> {
    let preview = preview_repair(paths)?;
    if preview.expected_hash != expected_hash {
        return Err("修复目标已发生变化，请重新预览。".into());
    }
    if let Some(id) = preview.profile_id {
        apply_profile(paths, &id, expected_hash)
    } else if let Some(id) = preview.backup_id {
        restore_backup(paths, &id, expected_hash)
    } else {
        crate::catalog::apply(paths, None, expected_hash)
    }
}
