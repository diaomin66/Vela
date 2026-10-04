//! Thin IPC and timer adapters for the standalone evaluation service.
use crate::evaluation::{types::*, EvaluationState};
use tauri::{Manager, State};

pub(super) fn start_scheduler(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            handle.state::<EvaluationState>().tick();
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });
}
#[tauri::command]
pub(super) fn get_evaluation_dashboard(
    state: State<'_, EvaluationState>,
) -> Result<EvaluationDashboard, String> {
    state.dashboard()
}
#[tauri::command]
pub(super) fn get_evaluation_activity(
    state: State<'_, EvaluationState>,
) -> Result<EvaluationActivity, String> {
    state.activity()
}
#[tauri::command]
pub(super) fn save_evaluation_plan(
    state: State<'_, EvaluationState>,
    plan: EvaluationPlan,
) -> Result<EvaluationDashboard, String> {
    state.save_plan(plan)
}
#[tauri::command]
pub(super) async fn start_evaluation(
    state: State<'_, EvaluationState>,
    plan: EvaluationPlan,
) -> Result<EvaluationDashboard, String> {
    state.start(plan, RunTrigger::Manual)
}
#[tauri::command]
pub(super) fn cancel_evaluation(
    state: State<'_, EvaluationState>,
    run_id: String,
) -> Result<EvaluationDashboard, String> {
    state.cancel(&run_id)
}
#[tauri::command]
pub(super) fn get_evaluation_run(
    state: State<'_, EvaluationState>,
    run_id: String,
) -> Result<EvaluationRun, String> {
    state.run(&run_id)
}
#[tauri::command]
pub(super) fn delete_evaluation_runs(
    state: State<'_, EvaluationState>,
    run_ids: Vec<String>,
) -> Result<EvaluationDashboard, String> {
    state.remove(&run_ids)
}
#[tauri::command]
pub(super) fn export_evaluation_run(
    state: State<'_, EvaluationState>,
    run_id: String,
) -> Result<EvaluationExport, String> {
    state.export(&run_id)
}
