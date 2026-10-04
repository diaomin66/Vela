//! Shared application state and bounded cancellation bookkeeping.
use crate::{
    core::{self, AppPaths},
    gateway,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};
use tokio::sync::watch;

pub(super) struct AppState {
    pub(super) paths: AppPaths,
    cancellations: Mutex<RunRegistry>,
    pub(super) gateway: tokio::sync::Mutex<Option<gateway::GatewayHandle>>,
    pub(super) gateway_error: Mutex<Option<String>>,
}
#[derive(Default)]
struct RunRegistry {
    active: HashMap<String, watch::Sender<bool>>,
    cancelled: VecDeque<String>,
}

impl AppState {
    pub(super) fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            cancellations: Mutex::new(RunRegistry::default()),
            gateway: tokio::sync::Mutex::new(None),
            gateway_error: Mutex::new(None),
        }
    }
}

/// Removes a finished or dropped request without relying on every error path to
/// remember cleanup. Dropping the sender also interrupts an in-flight receiver.
pub(super) struct ActiveRun<'a> {
    state: &'a AppState,
    id: String,
}

impl ActiveRun<'_> {
    pub(super) fn id(&self) -> &str {
        &self.id
    }
}

impl Drop for ActiveRun<'_> {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.state.cancellations.lock() {
            registry.active.remove(&self.id);
        }
    }
}

pub(super) fn register_run(
    state: &AppState,
    run_id: Option<String>,
) -> Result<(ActiveRun<'_>, watch::Receiver<bool>), String> {
    let id = run_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    core::validate_id(&id)?;
    let (sender, receiver) = watch::channel(false);
    let mut registry = state
        .cancellations
        .lock()
        .map_err(|_| "无法创建诊断任务。")?;
    if let Some(index) = registry
        .cancelled
        .iter()
        .position(|cancelled| cancelled == &id)
    {
        registry.cancelled.remove(index);
        return Err("验证已取消，未发起网络请求。".into());
    }
    if registry.active.contains_key(&id) {
        return Err("此诊断任务已经运行。".into());
    }
    if registry.active.len() >= 4 {
        return Err("同时运行的诊断过多，请取消已有任务后重试。".into());
    }
    registry.active.insert(id.clone(), sender);
    Ok((ActiveRun { state, id }, receiver))
}

pub(super) fn cancel_run(state: &AppState, run_id: String) -> Result<(), String> {
    core::validate_id(&run_id)?;
    let mut registry = state.cancellations.lock().map_err(|_| "无法取消任务。")?;
    if let Some(sender) = registry.active.get(&run_id) {
        let _ = sender.send(true);
    } else if !registry.cancelled.contains(&run_id) {
        // A close/cancel event can reach IPC before the request starts. Keep a
        // bounded tombstone so it cannot turn into a paid request a moment later.
        if registry.cancelled.len() >= 32 {
            registry.cancelled.pop_front();
        }
        registry.cancelled.push_back(run_id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        // No filesystem or credential access is needed for task accounting.
        AppState::new(AppPaths {
            data: "unused-test-data".into(),
            config: "unused-test-config".into(),
            helper: "unused-test-helper".into(),
            locations: None,
        })
    }

    #[test]
    fn dropping_a_request_releases_its_slot_and_cancellation_sender() {
        let state = state();
        let id = uuid::Uuid::new_v4().to_string();
        let (request, receiver) = register_run(&state, Some(id.clone())).unwrap();
        assert_eq!(request.id(), id);
        assert!(register_run(&state, Some(id.clone())).is_err());
        drop(request);
        assert!(receiver.has_changed().is_err());
        assert!(register_run(&state, Some(id)).is_ok());
    }

    #[test]
    fn cancellation_before_ipc_registration_never_starts_a_request() {
        let state = state();
        let id = uuid::Uuid::new_v4().to_string();
        cancel_run(&state, id.clone()).unwrap();
        assert!(register_run(&state, Some(id)).is_err());
        assert!(state.cancellations.lock().unwrap().active.is_empty());
    }

    #[test]
    fn running_requests_receive_cancellation_and_parallelism_stays_bounded() {
        let state = state();
        let mut requests = Vec::new();
        for _ in 0..4 {
            requests.push(register_run(&state, None).unwrap());
        }
        assert!(register_run(&state, None).is_err());
        cancel_run(&state, requests[0].0.id().into()).unwrap();
        assert!(*requests[0].1.borrow());
        requests.pop();
        assert!(register_run(&state, None).is_ok());
        for _ in 0..40 {
            cancel_run(&state, uuid::Uuid::new_v4().to_string()).unwrap();
        }
        assert_eq!(state.cancellations.lock().unwrap().cancelled.len(), 32);
    }
}
