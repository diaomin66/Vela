//! Task evaluations with private credentials, bounded history and one shared run slot.
mod cases;
mod client;
mod runner;
mod scheduler;
mod storage;
pub mod types;

use crate::core::{self, AppPaths};
use chrono::Utc;
use client::PreparedTarget;
use runner::PreparedPlan;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;
pub use types::*;

#[derive(Clone)]
pub(crate) struct EvaluationState {
    paths: AppPaths,
    runtime: Arc<Mutex<Runtime>>,
}
#[derive(Default)]
struct Runtime {
    active: Option<EvaluationRun>,
    cancel: Option<watch::Sender<bool>>,
    error: Option<String>,
}

fn validate_shape(plan: &EvaluationPlan, required: bool) -> Result<(), String> {
    if plan.targets.len() > 6 || (required && plan.targets.is_empty()) {
        return Err("请选择 1–6 个评测模型。".into());
    }
    if plan.cases.is_empty()
        || plan.cases.len() > 3
        || plan.cases.iter().copied().collect::<HashSet<_>>().len() != plan.cases.len()
    {
        return Err("请选择不同的评测题目。".into());
    }
    if !(1..=168).contains(&plan.interval_hours) {
        return Err("定时间隔需为 1–168 小时。".into());
    }
    let mut seen = HashSet::new();
    for target in plan.targets.iter().chain(plan.judge.iter()) {
        core::validate_id(&target.profile_id)?;
        if target.model_id.is_empty()
            || target.model_id.len() > 240
            || target.model_id.chars().any(char::is_control)
        {
            return Err("评测模型 ID 无效。".into());
        }
        if target.reasoning_effort.as_deref() == Some("ultra") {
            return Err("Ultra 需要原生多代理执行；评测请选择具体 API 推理档位。".into());
        }
    }
    for target in &plan.targets {
        if !seen.insert((&target.profile_id, &target.model_id)) {
            return Err("同一渠道的模型每轮只能选择一次；不同推理档位请分轮对比。".into());
        }
    }
    Ok(())
}
fn validate_saved_targets(paths: &AppPaths, plan: &EvaluationPlan) -> Result<(), String> {
    let store = core::load_store(paths)?;
    for target in plan.targets.iter().chain(plan.judge.iter()) {
        let profile = store
            .profiles
            .iter()
            .find(|profile| profile.id == target.profile_id)
            .ok_or("评测渠道已被删除，请重新选择。")?;
        let model = profile
            .models
            .iter()
            .find(|model| model.id == target.model_id && model.enabled)
            .ok_or("评测模型已停用或删除，请重新选择。")?;
        crate::reasoning::validate_api_effort(model, target.reasoning_effort.as_deref())?;
    }
    Ok(())
}
fn prepare_target(paths: &AppPaths, target: &EvaluationTarget) -> Result<PreparedTarget, String> {
    let (profile, key) = core::load_validation_profile(paths, &target.profile_id)?;
    let model = profile
        .models
        .iter()
        .find(|model| model.id == target.model_id && model.enabled)
        .ok_or("评测模型已停用或删除，请重新选择。")?;
    crate::reasoning::validate_api_effort(model, target.reasoning_effort.as_deref())?;
    let endpoint = crate::diagnostics::normalize_endpoint(
        profile
            .resolved_base_url
            .as_deref()
            .unwrap_or(&profile.base_url),
    )?;
    Ok(PreparedTarget {
        target: target.clone(),
        channel_name: profile.name.clone(),
        model_alias: model.alias.clone(),
        endpoint,
        key,
    })
}

impl EvaluationState {
    pub(crate) fn new(paths: AppPaths) -> Self {
        let error = storage::recover(&paths).err();
        Self {
            paths,
            runtime: Arc::new(Mutex::new(Runtime {
                error,
                ..Runtime::default()
            })),
        }
    }
    pub(crate) fn dashboard(&self) -> Result<EvaluationDashboard, String> {
        let runtime = self.runtime.lock().map_err(|_| "评测状态不可用。")?;
        let store = storage::read(&self.paths)?;
        Ok(EvaluationDashboard {
            plan: store.plan,
            cases: cases::definitions(),
            active: runtime.active.clone(),
            history: store.history,
            next_run_at: store.next_run_at,
            error: runtime.error.clone().or(store.error),
        })
    }
    pub(crate) fn save_plan(&self, plan: EvaluationPlan) -> Result<EvaluationDashboard, String> {
        // Always allow disabling a stale plan, even when its channels were removed.
        validate_shape(&plan, plan.schedule_enabled)?;
        if plan.schedule_enabled {
            validate_saved_targets(&self.paths, &plan)?;
        }
        {
            let _runtime = self.runtime.lock().map_err(|_| "评测状态不可用。")?;
            let _lock = self.paths.lock()?;
            let mut store = storage::read(&self.paths)?;
            store.next_run_at = plan
                .schedule_enabled
                .then(|| scheduler::next_time(Utc::now(), plan.interval_hours));
            store.plan = plan;
            store.error = None;
            storage::write(&self.paths, &store)?;
        }
        self.dashboard()
    }
    pub(crate) fn start(
        &self,
        plan: EvaluationPlan,
        trigger: RunTrigger,
    ) -> Result<EvaluationDashboard, String> {
        {
            let mut runtime = self.runtime.lock().map_err(|_| "评测状态不可用。")?;
            if runtime.active.is_some() {
                return Err("已有评测正在运行，请等待完成或取消。".into());
            }
            if trigger == RunTrigger::Scheduled {
                let current = storage::read(&self.paths)?.plan;
                if !current.schedule_enabled || current != plan {
                    drop(runtime);
                    return self.dashboard();
                }
            }
            validate_shape(&plan, true)?;
            let prepared = PreparedPlan {
                targets: plan
                    .targets
                    .iter()
                    .map(|target| prepare_target(&self.paths, target))
                    .collect::<Result<_, _>>()?,
                judge: plan
                    .judge
                    .as_ref()
                    .map(|target| prepare_target(&self.paths, target))
                    .transpose()?,
            };
            let run = EvaluationRun {
                id: uuid::Uuid::new_v4().to_string(),
                trigger,
                status: RunStatus::Running,
                started_at: Utc::now().to_rfc3339(),
                finished_at: None,
                case_version: cases::CASE_VERSION.into(),
                completed_cases: 0,
                total_cases: plan.targets.len() * plan.cases.len(),
                target_count: plan.targets.len(),
                plan,
                results: Vec::new(),
                error: None,
            };
            storage::register(&self.paths, &run)?;
            let (sender, receiver) = watch::channel(false);
            runtime.cancel = Some(sender);
            runtime.active = Some(run.clone());
            runtime.error = None;
            let state = self.clone();
            tokio::spawn(async move {
                let run = runner::execute(prepared, run, receiver, |run| state.progress(run)).await;
                state.finished(run);
            });
        }
        self.dashboard()
    }
    fn progress(&self, run: &EvaluationRun) -> Result<(), String> {
        let mut runtime = self.runtime.lock().map_err(|_| "评测状态不可用。")?;
        runtime.active = Some(run.clone());
        storage::write_run(&self.paths, run)
    }
    fn finished(&self, run: EvaluationRun) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.error = storage::finish(&self.paths, &run).err();
            runtime.active = None;
            runtime.cancel = None;
        }
    }
    pub(crate) fn cancel(&self, id: &str) -> Result<EvaluationDashboard, String> {
        {
            let runtime = self.runtime.lock().map_err(|_| "评测状态不可用。")?;
            if runtime.active.as_ref().is_some_and(|run| run.id == id) {
                if let Some(sender) = &runtime.cancel {
                    let _ = sender.send(true);
                }
            }
        }
        self.dashboard()
    }
    pub(crate) fn run(&self, id: &str) -> Result<EvaluationRun, String> {
        core::validate_id(id)?;
        let runtime = self.runtime.lock().map_err(|_| "评测状态不可用。")?;
        if let Some(run) = runtime.active.as_ref().filter(|run| run.id == id) {
            return Ok(run.clone());
        }
        storage::read_run(&self.paths, id)
    }
    pub(crate) fn export(&self, id: &str) -> Result<EvaluationExport, String> {
        storage::export(&self.paths, id)
    }
    pub(crate) fn tick(&self) {
        // Holding the shared slot through claim prevents two scheduler ticks
        // from advancing/starting the same due occurrence in one process.
        let plan = {
            let Ok(runtime) = self.runtime.lock() else {
                return;
            };
            if runtime.active.is_some() {
                return;
            }
            match scheduler::claim(&self.paths, Utc::now()) {
                Ok(plan) => plan,
                Err(_) => None,
            }
        };
        if let Some(plan) = plan {
            if let Err(error) = self.start(plan, RunTrigger::Scheduled) {
                if let Ok(mut runtime) = self.runtime.lock() {
                    runtime.error = Some(error);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
