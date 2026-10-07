use super::{item, DiagnosticItem};
use crate::{catalog, core};
use serde_json::Value;
use std::{fs, io::Read};
use toml_edit::{DocumentMut, Item};

const MAX_CATALOG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RECOVERY_CHECKS: usize = 20;

pub(super) fn inspect_document_shape(document: &DocumentMut, items: &mut Vec<DiagnosticItem>) {
    for (key, title) in [
        ("profiles", "配置方案列表格式错误"),
        ("model_providers", "服务商列表格式错误"),
    ] {
        if document.get(key).is_some_and(|value| !value.is_table_like()) {
            items.push(item(
                &format!("{key}-shape"),
                "configuration",
                title,
                "error",
                &format!("{key} 必须是配置表，当前值无法用于读取对应配置。"),
                Some("在恢复页面选择可用备份，或修正该配置表后重新检查。"),
            ));
        }
    }
    let profile = document
        .get("profile")
        .and_then(Item::as_str)
        .and_then(|name| document.get("profiles").and_then(|profiles| profiles.get(name)));
    if profile.is_some_and(|value| !value.is_table_like()) {
        items.push(item(
            "selected-profile-shape",
            "configuration",
            "当前配置方案格式错误",
            "error",
            "选中的配置方案不是配置表，无法正确应用其中的模型与服务商。",
            Some("在恢复页面选择可用备份，或修正选中的配置方案。"),
        ));
    }
    for (key, title) in [
        ("model_catalog_json", "模型目录路径格式错误"),
        ("model_reasoning_effort", "推理强度格式错误"),
    ] {
        let configured = profile
            .and_then(|value| value.get(key))
            .or_else(|| document.get(key));
        if configured.is_some_and(|value| value.as_str().is_none()) {
            items.push(item(
                &format!("{key}-type"),
                "configuration",
                title,
                "error",
                &format!("{key} 必须是字符串。"),
                Some("受管模型可通过重新同步模型目录修复；其他配置请修改此字段或恢复备份。"),
            ));
        }
    }
    if document.get("forced_login_method").is_some_and(|value| {
        !matches!(value.as_str(), Some("chatgpt" | "api"))
    }) {
        items.push(item(
            "forced-login-method",
            "authentication",
            "登录方式配置无效",
            "error",
            "forced_login_method 仅支持 chatgpt 或 api。",
            Some("根据所需登录方式修正该字段，或移除不需要的登录限制。"),
        ));
    }
}

pub(crate) fn inspect_local_system(
    paths: &core::AppPaths,
    store: &core::Store,
    gateway_configured: bool,
) -> Vec<DiagnosticItem> {
    let mut items = Vec::new();
    if store.profiles.iter().any(|profile| {
        profile.models.iter().any(|model| {
            model.enabled && catalog::is_internal_route_id(&model.id)
        })
    }) {
        items.push(item(
            "gateway-upstream-model",
            "configuration",
            "渠道误用了内部模型编号",
            "error",
            "已启用的渠道模型包含内部路由编号，无法作为第三方 API 的真实模型名称。",
            Some("重新拉取渠道模型，或在模型设置中填写真实模型 ID，再同步模型目录。"),
        ));
    }
    if fs::metadata(&paths.config).is_ok_and(|metadata| metadata.permissions().readonly()) {
        items.push(item(
            "config-readonly",
            "environment",
            "配置文件为只读",
            "error",
            "配置文件启用了只读属性，应用配置或恢复备份可能失败。",
            Some("在文件属性中取消只读后重新检查。ahaX 不会自动修改文件权限。"),
        ));
    }
    if gateway_configured {
        if !paths.helper.is_file() {
            items.push(item(
                "gateway-helper",
                "authentication",
                "网关凭据读取程序缺失",
                "error",
                "模型目录使用的凭据读取程序已不存在，客户端无法取得本地网关凭据。",
                Some("重新安装完整的 ahaX 安装包，再预览修复。"),
            ));
        }
        let expected = catalog::model_json(store);
        let models = expected["models"].as_array().map_or(0, Vec::len);
        if models == 0 {
            items.push(item(
                "gateway-models-empty",
                "configuration",
                "没有已启用模型",
                "error",
                "当前渠道没有可供统一模型目录使用的已启用模型。",
                Some("在模型库中启用至少一个模型，再重新同步。"),
            ));
        }
        let path = catalog::model_file(paths, store);
        let actual = fs::File::open(&path).ok().and_then(|file| {
            let mut bytes = Vec::new();
            file.take(MAX_CATALOG_BYTES + 1).read_to_end(&mut bytes).ok()?;
            if bytes.len() as u64 > MAX_CATALOG_BYTES {
                return None;
            }
            serde_json::from_slice::<Value>(&bytes).ok()
        });
        let matching = actual.as_ref() == Some(&expected);
        let mut diagnostic = item(
            "gateway-catalog-file",
            "configuration",
            if matching { "模型目录文件完整" } else { "模型目录文件需要重建" },
            if matching { "passed" } else { "error" },
            if matching {
                "已核对本机模型目录内容，与保存的渠道和模型一致。"
            } else {
                "ahaX 管理的模型目录缺失、损坏或与保存记录不同，客户端可能无法列出模型。"
            },
            (!matching).then_some("预览修复会根据已保存的渠道重新生成模型目录，并备份当前配置。"),
        );
        diagnostic.repairable = !matching;
        items.push(diagnostic);
    }
    let backups: Vec<_> = store.backups.iter().filter(|backup| {
        backup.config_path == paths.config.to_string_lossy()
    }).take(MAX_RECOVERY_CHECKS).collect();
    if backups.is_empty() {
        items.push(item(
            "recovery-point",
            "recovery",
            "尚无当前配置的恢复点",
            "info",
            "第一次应用配置时会自动创建备份。其他配置目录的备份不会用于当前目录。",
            None,
        ));
    } else {
        let verified = backups.iter().any(|backup| core::preview_restore(paths, &backup.id).is_ok());
        items.push(item(
            "recovery-point",
            "recovery",
            if verified { "恢复点已验证" } else { "最近的恢复点暂不可用" },
            if verified { "passed" } else { "warning" },
            if verified {
                "已找到可读取、可解析且与当前连接兼容的备份，可在恢复页面预览。"
            } else {
                "最近最多 20 份备份未通过恢复预检，可能已丢失、损坏，或引用了变化的连接。"
            },
            (!verified).then_some("优先重新应用已保存的连接；也可在恢复页面查看其他历史备份。"),
        ));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, core::AppPaths, core::Store) {
        let directory = tempfile::tempdir().unwrap();
        let paths = core::AppPaths {
            data: directory.path().join("data"),
            config: directory.path().join("config.toml"),
            helper: directory.path().join("helper.exe"),
            locations: None,
        };
        (directory, paths, core::Store::default())
    }

    #[test]
    fn stored_internal_models_produce_an_actionable_local_error() {
        let (_directory, paths, _) = fixture();
        for prefix in ["vela-", "ahax-"] {
            let store = serde_json::from_value(serde_json::json!({
                "profiles": [{"id":"11111111-1111-4111-8111-111111111111", "name":"fixture", "baseUrl":"https://example.test/v1", "model":"upstream", "keyStored":false, "createdAt":"now", "updatedAt":"now", "models":[{"id":format!("{prefix}{}", "a".repeat(64)), "enabled":true}]}]
            })).unwrap();
            let items = inspect_local_system(&paths, &store, false);
            let invalid = items.iter().find(|item| item.id == "gateway-upstream-model").unwrap();
            assert_eq!(invalid.status, "error");
            assert!(!invalid.repairable);
            assert!(invalid.action.as_deref().unwrap().contains("真实模型"));
        }
    }

    #[test]
    fn malformed_tables_and_effective_field_types_are_reported_without_values() {
        let document = "profiles=12\nmodel_providers=false\nmodel_catalog_json=42\nmodel_reasoning_effort=7\nforced_login_method='secret-invalid-value'".parse().unwrap();
        let mut items = Vec::new();
        inspect_document_shape(&document, &mut items);
        assert_eq!(items.len(), 5);
        assert!(items.iter().all(|item| item.status == "error"));
        assert!(!serde_json::to_string(&items).unwrap().contains("secret-invalid-value"));
        let document = "profile='work'\nmodel_reasoning_effort=1\n[profiles.work]\nmodel_reasoning_effort='custom-future-effort'".parse().unwrap();
        let mut items = Vec::new();
        inspect_document_shape(&document, &mut items);
        assert!(items.is_empty());
    }

    #[test]
    fn managed_catalog_corruption_is_repairable_and_foreign_files_are_untouched() {
        let (_directory, paths, store) = fixture();
        fs::write(&paths.config, b"model_catalog_json='foreign.json'").unwrap();
        let foreign = paths.config.parent().unwrap().join("foreign.json");
        fs::write(&foreign, b"private foreign catalog").unwrap();
        let initial = inspect_local_system(&paths, &store, false);
        assert!(!initial.iter().any(|item| item.id == "gateway-catalog-file"));
        catalog::write_model_catalog(&paths, &store).unwrap();
        let valid = inspect_local_system(&paths, &store, true);
        assert!(valid.iter().any(|item| item.id == "gateway-catalog-file" && item.status == "passed"));
        fs::write(catalog::model_file(&paths, &store), b"broken private data").unwrap();
        let invalid = inspect_local_system(&paths, &store, true);
        assert!(invalid.iter().any(|item| item.id == "gateway-catalog-file" && item.repairable));
        assert!(!serde_json::to_string(&invalid).unwrap().contains("broken private data"));
        assert_eq!(fs::read(&foreign).unwrap(), b"private foreign catalog");
    }

    #[test]
    fn diagnosis_preserves_readonly_configuration_and_flags_it_for_manual_action() {
        let (_directory, paths, store) = fixture();
        fs::write(&paths.config, b"model='untouched'").unwrap();
        let original = fs::metadata(&paths.config).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&paths.config, readonly).unwrap();
        let items = inspect_local_system(&paths, &store, false);
        assert!(items.iter().any(|item| item.id == "config-readonly" && !item.repairable));
        assert!(fs::metadata(&paths.config).unwrap().permissions().readonly());
        assert_eq!(fs::read(&paths.config).unwrap(), b"model='untouched'");
        fs::set_permissions(&paths.config, original).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn recovery_check_verifies_encrypted_backup_and_detects_corruption() {
        let (_directory, paths, _) = fixture();
        fs::write(&paths.config, b"model='before'").unwrap();
        let backup = core::commit_config(&paths, core::read_config(&paths).unwrap(), Some(b"model='after'"), "fixture", "fixture").unwrap();
        let store = core::load_store(&paths).unwrap();
        assert!(inspect_local_system(&paths, &store, false).iter().any(|item| item.id == "recovery-point" && item.status == "passed"));
        let backup_path = paths.data.join("backups").join(format!("{}.bin", backup.id));
        fs::write(backup_path, b"corrupt encrypted snapshot").unwrap();
        assert!(inspect_local_system(&paths, &store, false).iter().any(|item| item.id == "recovery-point" && item.status == "warning"));
        assert_eq!(fs::read(&paths.config).unwrap(), b"model='after'");
    }
}
