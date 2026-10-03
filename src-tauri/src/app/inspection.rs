//! Assemble local diagnostics and explicitly requested paid compatibility checks.
use super::state::{register_run, AppState};
use crate::{catalog, core, diagnostics, security};
use zeroize::Zeroizing;

pub(super) async fn validate_profile(
    state: &AppState,
    id: String,
    run_id: Option<String>,
    model_id: Option<String>,
) -> Result<diagnostics::ValidationResult, String> {
    let (_run, receiver) = register_run(state, run_id)?;
    let (profile, key) = core::load_validation_profile(&state.paths, &id)?;
    let result = diagnostics::validate_connection(
        diagnostics::ValidationInput {
            endpoint: profile
                .resolved_base_url
                .clone()
                .unwrap_or(profile.base_url),
            model: model_id.unwrap_or(profile.model),
            key: key.to_string(),
        },
        receiver,
    )
    .await;
    if !core::mark_validated(&state.paths, &id, &profile.revision, result.ok)? {
        return Err("验证期间连接已被编辑或删除，请重新验证当前连接。".into());
    }
    Ok(result)
}

pub(super) async fn run_diagnostics(
    state: &AppState,
    run_id: Option<String>,
    include_network: Option<bool>,
) -> Result<diagnostics::DiagnosticReport, String> {
    let store = core::load_store(&state.paths)?;
    let config = core::read_config(&state.paths);
    let text = match &config {
        Ok(bytes) => core::config_text(bytes).ok().map(str::to_owned),
        Err(_) => None,
    };
    let active = text
        .as_deref()
        .and_then(|s| core::active_profile_id(s, &store.profiles))
        .and_then(|id| store.profiles.iter().find(|p| p.id == id));
    let key = active.map(|p| security::get_secret(&p.id).map(Zeroizing::new));
    let local = diagnostics::LocalDiagnosticInput {
        config_path: state.paths.config.to_string_lossy().into(),
        config_contents: if config.as_ref().ok().is_some_and(|v| v.is_none()) {
            None
        } else {
            text
        },
        config_read_error: config.is_err()
            || config
                .as_ref()
                .ok()
                .is_some_and(|v| core::config_text(v).is_err()),
        expected_provider: active.map(|p| core::provider_id(&p.id)),
        expected_model: active.map(|p| p.model.clone()),
        expected_endpoint: active.map(|p| p.base_url.clone()),
        expected_helper_path: active.map(|_| state.paths.helper.to_string_lossy().into_owned()),
        expected_profile_id: active.map(|p| p.id.clone()),
        credential_available: key.as_ref().map(Result::is_ok),
        credential_helper_available: active.map(|_| state.paths.helper.is_file()),
    };
    let mut report = diagnostics::inspect_configuration(local);
    let config_text = config
        .as_ref()
        .ok()
        .and_then(|bytes| core::config_text(bytes).ok())
        .unwrap_or("");
    if catalog::is_gateway_config(config_text) {
        let running = state
            .gateway
            .lock()
            .await
            .as_ref()
            .is_some_and(|handle| handle.status().running);
        report.items.push(diagnostics::DiagnosticItem {
            id: "gateway-runtime".into(),
            category: "gateway".into(),
            title: if running {
                "本地网关正在运行"
            } else {
                "本地网关尚未就绪"
            }
            .into(),
            status: if running { "passed" } else { "error" }.into(),
            description: if running {
                "窗口关闭后网关仍在托盘运行，使用独立本地凭据认证。"
            } else {
                "本地转发服务未运行，请检查端口占用或重新启动 Vela。"
            }
            .into(),
            action: None,
            repairable: !running,
        });
        let synchronized = catalog::applied(&state.paths, &store, config_text);
        report.items.push(diagnostics::DiagnosticItem {
            id: "gateway-catalog".into(),
            category: "configuration".into(),
            title: if synchronized {
                "模型目录与保存记录一致"
            } else {
                "模型目录有更改待同步"
            }
            .into(),
            status: if synchronized { "passed" } else { "warning" }.into(),
            description: if synchronized {
                "当前配置与模型目录对应所有已启用模型。"
            } else {
                "保存的模型或网关设置与已应用配置不同；请预览并同步模型目录。"
            }
            .into(),
            action: None,
            repairable: !synchronized,
        });
        for profile in &store.profiles {
            if profile.models.iter().any(|model| model.enabled)
                && security::get_secret(&profile.id)
                    .map(Zeroizing::new)
                    .is_err()
            {
                report.items.push(diagnostics::DiagnosticItem {
                    id: format!("gateway-key-{}", profile.id),
                    category: "authentication".into(),
                    title: "已启用渠道的凭据缺失".into(),
                    status: "error".into(),
                    description: "至少一个启用模型所属渠道无法从系统凭据库读取 Key。".into(),
                    action: Some("编辑对应渠道并重新保存 Key。".into()),
                    repairable: false,
                });
            }
        }
    }
    let (run, receiver) = register_run(state, run_id)?;
    report.id = run.id().into();
    if let (true, Some(profile), Some(Ok(_))) = (include_network.unwrap_or(false), active, key) {
        let (profile, key) = core::load_validation_profile(&state.paths, &profile.id)?;
        let validation = diagnostics::validate_connection(
            diagnostics::ValidationInput {
                endpoint: profile.base_url.clone(),
                model: profile.model.clone(),
                key: key.to_string(),
            },
            receiver,
        )
        .await;
        report.items.retain(|item| item.id != "inspection-scope");
        report.items.extend(validation.items);
    }
    let errors = report
        .items
        .iter()
        .filter(|item| item.status == "error")
        .count();
    let warnings = report
        .items
        .iter()
        .filter(|item| item.status == "warning")
        .count();
    report.can_repair = report.items.iter().any(|item| item.repairable)
        && core::preview_repair(&state.paths).is_ok();
    report.summary = if errors > 0 {
        format!("发现 {errors} 项需要处理的问题和 {warnings} 项提醒。")
    } else if warnings > 0 {
        format!("检查完成，有 {warnings} 项需要关注。")
    } else {
        "检查完成，已检查项目运行正常。".into()
    };
    Ok(report)
}
