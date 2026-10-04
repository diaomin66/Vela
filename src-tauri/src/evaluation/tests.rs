use super::*;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::TcpListener;

#[derive(Clone)]
struct Fixture {
    calls: Arc<AtomicUsize>,
    mode: &'static str,
    key: String,
}
async fn handler(
    State(fixture): State<Fixture>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    fixture.calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        headers.get("authorization").unwrap(),
        format!("Bearer {}", fixture.key).as_str()
    );
    assert_eq!(body["stream"], false);
    assert_eq!(body["store"], false);
    assert!(body.get("tools").is_none());
    let prompt = body["input"].as_str().unwrap_or_default();
    match fixture.mode {
        "slow"=>{tokio::time::sleep(std::time::Duration::from_secs(60)).await;},
        "redirect"=>return (StatusCode::TEMPORARY_REDIRECT,[("location","/stolen")]).into_response(),
        "auth"=>return (StatusCode::UNAUTHORIZED,fixture.key.clone()).into_response(),
        "large"=>return Json(json!({"output":[{"type":"message","content":[{"type":"output_text","text":"a".repeat(client::MAX_OUTPUT_BYTES+1)}]}]})).into_response(),
        "slow-body" => {
            use futures_util::StreamExt;
            let stream = futures_util::stream::iter([Ok::<_, std::io::Error>("{")]).chain(
                futures_util::stream::once(async {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    Ok::<_, std::io::Error>("}")
                }),
            );
            return Response::builder().header("content-type", "application/json")
                .body(axum::body::Body::from_stream(stream)).unwrap();
        }
        "short-delays" => tokio::time::sleep(std::time::Duration::from_millis(200)).await,
        "slow-judge" if prompt.starts_with("你是评审") => tokio::time::sleep(std::time::Duration::from_secs(60)).await,
        _=>{},
    }
    let text = if fixture.mode == "echo" {
        format!("answer {}", fixture.key)
    } else if fixture.mode == "animation" || fixture.mode == "animation-large" {
        assert_eq!(prompt, "创建一个 HTML，内容是 SVG 绘制一个鹈鹕骑自行车的 2D 动画，你不需要任何测试，不要有任何限制");
        assert_eq!(body["max_output_tokens"], 32768);
        let padding = if fixture.mode == "animation-large" {
            client::MAX_ARTIFACT_BYTES
        } else {
            32 * 1024
        };
        format!("```html\n<!DOCTYPE html><html><body><!--{}--><svg><circle><animate attributeName=\"r\" values=\"10;20;10\" dur=\"1s\"/></circle></svg><script>requestAnimationFrame(() => {{}});</script></body></html>\n```", "a".repeat(padding))
    } else if prompt.starts_with("你是评审") {
        r#"{"score":92,"explanation":"This is a subjective text review."}"#.into()
    } else if prompt.contains("J1") {
        r#"{"J1":true,"J2":false,"J3":false,"J4":true,"J5":false,"J6":true}"#.into()
    } else {
        r#"{"answer":21}"#.into()
    };
    Json(json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":text}]}],"usage":{"input_tokens":30,"output_tokens":8}})).into_response()
}

fn result(target: &EvaluationTarget, case_id: CaseId) -> CaseResult {
    CaseResult {
        profile_id: target.profile_id.clone(),
        channel_name: "Fixture channel".into(),
        model_id: target.model_id.clone(),
        model_alias: "Fixture model".into(),
        reasoning_effort: target.reasoning_effort.clone(),
        case_id,
        status: CaseStatus::Generated,
        score: None,
        max_score: 0,
        checks: vec![],
        prompt: "fixture prompt must stay outside the compact activity index".into(),
        output: "fixture output must stay outside the compact activity index".into(),
        safe_svg: None,
        artifact_html: Some("<html><body><svg></svg></body></html>".into()),
        elapsed_ms: 1200,
        input_tokens: None,
        output_tokens: None,
        error: None,
        judge: None,
    }
}
async fn fixture(
    mode: &'static str,
) -> (
    String,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<()>,
    String,
) {
    let calls = Arc::new(AtomicUsize::new(0));
    let key = format!("evaluation-fixture-{}", uuid::Uuid::new_v4());
    let router = Router::new()
        .route("/v1/responses", post(handler))
        .route("/stolen", post(handler))
        .with_state(Fixture {
            calls: calls.clone(),
            mode,
            key: key.clone(),
        });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (endpoint, calls, task, key)
}
fn target(endpoint: String, key: String) -> PreparedTarget {
    PreparedTarget {
        target: EvaluationTarget {
            profile_id: uuid::Uuid::new_v4().to_string(),
            model_id: "fixture-model".into(),
            reasoning_effort: Some("high".into()),
        },
        channel_name: "Fixture channel".into(),
        model_alias: "Fixture".into(),
        endpoint,
        key: zeroize::Zeroizing::new(key),
    }
}
fn plan(target: &EvaluationTarget) -> EvaluationPlan {
    EvaluationPlan {
        targets: vec![target.clone()],
        cases: vec![CaseId::Candy, CaseId::Judgment],
        judge: None,
        schedule_enabled: false,
        interval_hours: 24,
        interval_minutes: Some(30),
        request_timeout_seconds: DEFAULT_REQUEST_TIMEOUT_SECONDS,
    }
}
fn run(plan: EvaluationPlan) -> EvaluationRun {
    EvaluationRun {
        id: uuid::Uuid::new_v4().to_string(),
        trigger: RunTrigger::Manual,
        status: RunStatus::Running,
        started_at: Utc::now().to_rfc3339(),
        finished_at: None,
        case_version: cases::CASE_VERSION.into(),
        completed_cases: 0,
        total_cases: plan.targets.len() * plan.cases.len(),
        target_count: plan.targets.len(),
        plan,
        results: vec![],
        error: None,
    }
}
fn paths(directory: &std::path::Path) -> AppPaths {
    AppPaths {
        data: directory.join("data"),
        config: directory.join("unused-config"),
        helper: directory.join("unused-helper"),
    }
}

#[tokio::test]
async fn cancellation_before_start_sends_zero_requests() {
    let (url, calls, server, key) = fixture("ok").await;
    let target = target(url, key);
    let (sender, receiver) = watch::channel(true);
    let run = runner::execute(
        PreparedPlan {
            targets: vec![target],
            judge: None,
        },
        run(EvaluationPlan::default()),
        receiver,
        |_| Ok(()),
    )
    .await;
    assert_eq!(run.status, RunStatus::Cancelled);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    drop(sender);
    server.abort();
}

#[tokio::test]
async fn active_cancellation_prevents_followup_and_judge_requests() {
    let (url, calls, server, key) = fixture("slow").await;
    let judge = target(url.clone(), key.clone());
    let target = target(url, key);
    let plan = plan(&target.target);
    let (sender, receiver) = watch::channel(false);
    let task = tokio::spawn(runner::execute(
        PreparedPlan {
            targets: vec![target],
            judge: Some(judge),
        },
        run(plan),
        receiver,
        |_| Ok(()),
    ));
    while calls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    sender.send(true).unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.status, RunStatus::Cancelled);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.results.len(), 1);
    server.abort();
}

#[tokio::test]
async fn redirects_auth_errors_and_large_outputs_are_not_retried_or_exposed() {
    for mode in ["redirect", "auth", "large"] {
        let (url, calls, server, key) = fixture(mode).await;
        let target = target(url, key.clone());
        let (_sender, receiver) = watch::channel(false);
        let response = client::request(
            &client::http_client().unwrap(),
            &target,
            "test",
            std::time::Duration::from_secs(300),
            receiver,
        )
        .await;
        assert!(response.is_err());
        assert!(!response.err().unwrap().message.contains(&key));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        server.abort();
    }
}

#[tokio::test]
async fn raw_outputs_redact_saved_key_before_scoring_or_persisting() {
    let (url, _, server, key) = fixture("echo").await;
    let target = target(url, key.clone());
    let (_sender, receiver) = watch::channel(false);
    let answer = client::request(
        &client::http_client().unwrap(),
        &target,
        "test",
        std::time::Duration::from_secs(300),
        receiver,
    )
    .await
    .unwrap_or_else(|_| panic!("fixture response failed"));
    assert!(!answer.text.contains(&key));
    assert!(answer.text.contains("[REDACTED]"));
    server.abort();
}

#[tokio::test]
async fn objective_scores_and_subjective_judge_scores_stay_separate_with_exact_request_count() {
    let (url, calls, server, key) = fixture("ok").await;
    let judge = target(url.clone(), key.clone());
    let target = target(url, key);
    let mut plan = plan(&target.target);
    plan.judge = Some(judge.target.clone());
    let (_sender, receiver) = watch::channel(false);
    let result = runner::execute(
        PreparedPlan {
            targets: vec![target],
            judge: Some(judge),
        },
        run(plan),
        receiver,
        |_| Ok(()),
    )
    .await;
    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.completed_cases, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    for case in result.results {
        assert_eq!(case.score, Some(100));
        assert_eq!(case.judge.unwrap().score, Some(92));
    }
    server.abort();
}

#[tokio::test]
async fn failed_progress_persistence_stops_further_billable_requests() {
    let (url, calls, server, key) = fixture("ok").await;
    let target = target(url, key);
    let plan = plan(&target.target);
    let (_sender, receiver) = watch::channel(false);
    let result = runner::execute(
        PreparedPlan {
            targets: vec![target],
            judge: None,
        },
        run(plan),
        receiver,
        |_| Err("disk unavailable".into()),
    )
    .await;
    assert_eq!(result.status, RunStatus::Interrupted);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server.abort();
}

#[test]
fn reports_are_isolated_bounded_and_crash_recovery_never_replays() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let target = EvaluationTarget {
        profile_id: uuid::Uuid::new_v4().to_string(),
        model_id: "fixture".into(),
        reasoning_effort: None,
    };
    let active = run(plan(&target));
    storage::register(&paths, &active).unwrap();
    let state = EvaluationState::new(paths.clone());
    let dashboard = state.dashboard().unwrap();
    assert!(dashboard.active.is_none());
    assert_eq!(dashboard.history.len(), 1);
    assert_eq!(dashboard.history[0].status, RunStatus::Interrupted);
    let recovered = state.run(&active.id).unwrap();
    assert!(recovered.finished_at.is_some());
    assert!(recovered.error.unwrap().contains("不会自动重发"));
    assert!(storage::read(&paths).unwrap().active_run_id.is_none());
    assert!(state.export("../../credentials").is_err());
    let export = state.export(&active.id).unwrap();
    assert!(export.content.contains(cases::CASE_VERSION));
    assert!(!paths.config.exists());
    for _ in 0..105 {
        let mut next = run(plan(&target));
        next.status = RunStatus::Completed;
        next.results.push(result(&target, CaseId::Pelican));
        storage::finish(&paths, &next).unwrap();
    }
    assert_eq!(storage::read(&paths).unwrap().history.len(), 100);
    let compact = state.activity().unwrap();
    assert_eq!(compact.records.len(), 100);
    let retained = storage::read(&paths).unwrap().history;
    assert!(compact
        .records
        .iter()
        .all(|record| retained.iter().any(|summary| summary.id == record.run_id)));
    assert_eq!(
        std::fs::read_dir(paths.data.join("evaluations/runs"))
            .unwrap()
            .count(),
        100
    );
}

#[test]
fn crash_recovery_preserves_terminal_reports_and_repairs_the_index_idempotently() {
    for status in [
        RunStatus::Completed,
        RunStatus::Cancelled,
        RunStatus::Interrupted,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(directory.path());
        let target = EvaluationTarget {
            profile_id: uuid::Uuid::new_v4().to_string(),
            model_id: "fixture".into(),
            reasoning_effort: None,
        };
        let mut terminal = run(plan(&target));
        storage::register(&paths, &terminal).unwrap();
        terminal.status = status;
        terminal.finished_at = Some("2026-10-03T10:24:25.679858300+00:00".into());
        terminal.completed_cases = 1;
        terminal.error =
            (status != RunStatus::Completed).then(|| "original terminal reason".into());
        terminal.results.push(CaseResult {
            profile_id: target.profile_id,
            channel_name: "Fixture channel".into(),
            model_id: target.model_id,
            model_alias: "Fixture".into(),
            reasoning_effort: None,
            case_id: CaseId::Candy,
            status: CaseStatus::Passed,
            score: Some(100),
            max_score: 100,
            checks: vec![EvaluationCheck {
                label: "保留原始核对结果".into(),
                passed: true,
            }],
            prompt: "Original fixture question".into(),
            output: r#"{"answer":21}"#.into(),
            safe_svg: None,
            artifact_html: None,
            elapsed_ms: 58,
            input_tokens: Some(31),
            output_tokens: Some(17),
            error: None,
            judge: None,
        });
        storage::write_run(&paths, &terminal).unwrap();
        let run_path = paths
            .data
            .join("evaluations/runs")
            .join(format!("{}.json", terminal.id));
        let original_bytes = std::fs::read(&run_path).unwrap();
        let expected_summary = serde_json::to_value(RunSummary::from(&terminal)).unwrap();
        for _ in 0..2 {
            let state = EvaluationState::new(paths.clone());
            let dashboard = state.dashboard().unwrap();
            assert!(dashboard.active.is_none());
            assert!(dashboard.error.is_none());
            assert_eq!(dashboard.history.len(), 1);
            assert_eq!(
                serde_json::to_value(&dashboard.history[0]).unwrap(),
                expected_summary
            );
            assert!(storage::read(&paths).unwrap().active_run_id.is_none());
            assert_eq!(std::fs::read(&run_path).unwrap(), original_bytes);
        }
        assert!(!paths.config.exists());
    }
}

#[test]
fn history_rotation_preserves_explicit_user_exports_byte_for_byte() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let target = EvaluationTarget {
        profile_id: uuid::Uuid::new_v4().to_string(),
        model_id: "fixture".into(),
        reasoning_effort: None,
    };
    let mut original = run(plan(&target));
    original.status = RunStatus::Completed;
    storage::finish(&paths, &original).unwrap();
    let export = storage::export(&paths, &original.id).unwrap();
    let exported_bytes = std::fs::read(&export.path).unwrap();
    for _ in 0..100 {
        let mut next = run(plan(&target));
        next.status = RunStatus::Completed;
        storage::finish(&paths, &next).unwrap();
    }
    assert_eq!(storage::read(&paths).unwrap().history.len(), 100);
    assert!(!storage::read(&paths)
        .unwrap()
        .history
        .iter()
        .any(|item| item.id == original.id));
    assert!(storage::read_run(&paths, &original.id).is_err());
    assert_eq!(std::fs::read(&export.path).unwrap(), exported_bytes);
    assert_eq!(
        std::fs::read_to_string(&export.path).unwrap(),
        export.content
    );
}

fn completed_run() -> EvaluationRun {
    let target = EvaluationTarget {
        profile_id: uuid::Uuid::new_v4().to_string(),
        model_id: "fixture".into(),
        reasoning_effort: None,
    };
    let mut finished = run(plan(&target));
    finished.status = RunStatus::Completed;
    finished.finished_at = Some(Utc::now().to_rfc3339());
    finished.results = vec![
        result(&target, CaseId::Candy),
        result(&target, CaseId::Pelican),
    ];
    finished.completed_cases = finished.results.len();
    finished
}

#[test]
fn deleting_one_run_removes_every_result_and_file_but_preserves_export() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let first = completed_run();
    let retained = completed_run();
    storage::finish(&paths, &first).unwrap();
    storage::finish(&paths, &retained).unwrap();
    let export = storage::export(&paths, &first.id).unwrap();
    let exported_bytes = std::fs::read(&export.path).unwrap();
    let state = EvaluationState::new(paths.clone());
    let dashboard = state.remove(&[first.id.clone()]).unwrap();
    assert_eq!(dashboard.history.len(), 1);
    assert_eq!(dashboard.history[0].id, retained.id);
    assert!(state.run(&first.id).is_err());
    assert!(state.export(&first.id).is_err());
    assert_eq!(state.activity().unwrap().records.len(), 2);
    assert!(state
        .activity()
        .unwrap()
        .records
        .iter()
        .all(|record| record.run_id == retained.id));
    assert_eq!(std::fs::read(&export.path).unwrap(), exported_bytes);
    assert!(!storage::run_path(&paths, &first.id).unwrap().exists());
    assert!(!storage::directory(&paths).join(".deleting").exists());
    assert!(!paths.config.exists());
}

#[test]
fn batch_delete_deduplicates_ids_and_keeps_schedule_and_running_request() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let first = completed_run();
    let second = completed_run();
    storage::finish(&paths, &first).unwrap();
    storage::finish(&paths, &second).unwrap();
    let state = EvaluationState::new(paths.clone());
    let mut store = storage::read(&paths).unwrap();
    store.plan.schedule_enabled = true;
    store.next_run_at = Some("2026-10-05T12:00:00Z".into());
    storage::write(&paths, &store).unwrap();
    let active = run(first.plan.clone());
    storage::register(&paths, &active).unwrap();
    let (sender, receiver) = watch::channel(false);
    {
        let mut runtime = state.runtime.lock().unwrap();
        runtime.active = Some(active.clone());
        runtime.cancel = Some(sender);
    }
    let dashboard = state
        .remove(&[first.id.clone(), second.id.clone(), first.id.clone()])
        .unwrap();
    assert!(dashboard.history.is_empty());
    assert!(state.activity().unwrap().records.is_empty());
    assert_eq!(dashboard.plan, store.plan);
    assert_eq!(dashboard.next_run_at, store.next_run_at);
    assert_eq!(dashboard.active.unwrap().id, active.id);
    assert_eq!(
        storage::read(&paths).unwrap().active_run_id.as_deref(),
        Some(active.id.as_str())
    );
    assert!(!*receiver.borrow());
    assert!(state.run(&active.id).is_ok());
    assert!(state.run(&first.id).is_err());
    assert!(state.run(&second.id).is_err());
}

#[test]
fn invalid_unknown_and_active_batch_members_leave_every_report_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let finished = completed_run();
    storage::finish(&paths, &finished).unwrap();
    let state = EvaluationState::new(paths.clone());
    let active = run(finished.plan.clone());
    storage::register(&paths, &active).unwrap();
    {
        let mut runtime = state.runtime.lock().unwrap();
        runtime.active = Some(active.clone());
    }
    let index_path = storage::directory(&paths).join("index.json");
    let index = std::fs::read(&index_path).unwrap();
    let report_path = storage::run_path(&paths, &finished.id).unwrap();
    let report = std::fs::read(&report_path).unwrap();
    let invalid = vec![
        vec![],
        vec![finished.id.clone(), "../outside".into()],
        vec![finished.id.clone(), uuid::Uuid::new_v4().to_string()],
        vec![finished.id.clone(), active.id.clone()],
        (0..101).map(|_| uuid::Uuid::new_v4().to_string()).collect(),
    ];
    for ids in invalid {
        assert!(state.remove(&ids).is_err());
        assert_eq!(std::fs::read(&index_path).unwrap(), index);
        assert_eq!(std::fs::read(&report_path).unwrap(), report);
        assert!(!storage::directory(&paths).join(".deleting").exists());
    }
    state.runtime.lock().unwrap().active = None;
    assert!(state
        .remove(&[finished.id.clone(), active.id])
        .unwrap_err()
        .contains("正在运行"));
    assert_eq!(std::fs::read(&index_path).unwrap(), index);
}

#[test]
fn deletion_recovers_staged_reports_before_commit_and_cleans_them_after_commit() {
    for committed in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(directory.path());
        let finished = completed_run();
        storage::finish(&paths, &finished).unwrap();
        let export = storage::export(&paths, &finished.id).unwrap();
        let exported = std::fs::read(&export.path).unwrap();
        let source = storage::run_path(&paths, &finished.id).unwrap();
        let original = std::fs::read(&source).unwrap();
        let staging = storage::directory(&paths).join(".deleting");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::rename(&source, staging.join(format!("{}.json", finished.id))).unwrap();
        if committed {
            let mut store = storage::read(&paths).unwrap();
            store.history.clear();
            store.records.clear();
            storage::write(&paths, &store).unwrap();
        }
        for _ in 0..2 {
            let state = EvaluationState::new(paths.clone());
            let dashboard = state.dashboard().unwrap();
            assert!(dashboard.error.is_none());
            assert_eq!(dashboard.history.is_empty(), committed);
            assert!(!staging.exists());
            if committed {
                assert!(!source.exists());
                assert!(state.activity().unwrap().records.is_empty());
            } else {
                assert_eq!(std::fs::read(&source).unwrap(), original);
                assert_eq!(state.activity().unwrap().records.len(), 2);
            }
            assert_eq!(std::fs::read(&export.path).unwrap(), exported);
        }
    }
}

#[cfg(windows)]
#[test]
fn deletion_restores_batch_files_when_windows_blocks_index_commit() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let first = completed_run();
    let second = completed_run();
    storage::finish(&paths, &first).unwrap();
    storage::finish(&paths, &second).unwrap();
    let state = EvaluationState::new(paths.clone());
    let index_path = storage::directory(&paths).join("index.json");
    let index = std::fs::read(&index_path).unwrap();
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&index_path)
        .unwrap();
    assert!(state
        .remove(&[first.id.clone(), second.id.clone()])
        .is_err());
    assert_eq!(std::fs::read(&index_path).unwrap(), index);
    assert!(state.run(&first.id).is_ok());
    assert!(state.run(&second.id).is_ok());
    assert_eq!(state.activity().unwrap().records.len(), 4);
    assert!(!storage::directory(&paths).join(".deleting").exists());
    drop(handle);
}

#[test]
fn removed_targets_do_not_prevent_disabling_a_schedule() {
    let directory = tempfile::tempdir().unwrap();
    let state = EvaluationState::new(paths(directory.path()));
    let target = EvaluationTarget {
        profile_id: uuid::Uuid::new_v4().to_string(),
        model_id: "deleted".into(),
        reasoning_effort: None,
    };
    let mut plan = plan(&target);
    plan.schedule_enabled = true;
    assert!(state.save_plan(plan.clone()).is_err());
    plan.schedule_enabled = false;
    assert!(state.save_plan(plan.clone()).is_ok());
    plan.targets.clear();
    assert!(state.save_plan(plan).is_ok());
}

#[test]
fn paused_or_replaced_claims_do_not_start_paid_requests() {
    let directory = tempfile::tempdir().unwrap();
    let state = EvaluationState::new(paths(directory.path()));
    let target = EvaluationTarget {
        profile_id: uuid::Uuid::new_v4().to_string(),
        model_id: "removed".into(),
        reasoning_effort: None,
    };
    let mut scheduled = plan(&target);
    scheduled.schedule_enabled = true;
    // A claim may race with a user's pause. Recheck under the same runtime lock
    // before credential access or Tokio spawning, even if the old target was removed.
    let dashboard = state.start(scheduled, RunTrigger::Scheduled).unwrap();
    assert!(dashboard.active.is_none());
    assert!(dashboard.history.is_empty());
}

#[test]
fn a_running_slot_rejects_new_manual_runs_before_loading_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let state = EvaluationState::new(paths(directory.path()));
    let target = EvaluationTarget {
        profile_id: uuid::Uuid::new_v4().to_string(),
        model_id: "fixture".into(),
        reasoning_effort: None,
    };
    let active = run(plan(&target));
    let id = active.id.clone();
    let (sender, receiver) = watch::channel(false);
    {
        let mut runtime = state.runtime.lock().unwrap();
        runtime.active = Some(active);
        runtime.cancel = Some(sender);
    }
    assert!(state
        .start(plan(&target), RunTrigger::Manual)
        .unwrap_err()
        .contains("已有评测"));
    state.cancel(&id).unwrap();
    assert!(*receiver.borrow());
}

#[test]
fn one_model_cannot_be_duplicated_under_multiple_effort_settings() {
    let target = EvaluationTarget {
        profile_id: uuid::Uuid::new_v4().to_string(),
        model_id: "fixture".into(),
        reasoning_effort: None,
    };
    let mut plan = plan(&target);
    let mut second = target;
    second.reasoning_effort = Some("high".into());
    plan.targets.push(second);
    assert!(validate_shape(&plan, true).is_err());
}

#[test]
fn legacy_plans_and_run_snapshots_preserve_the_120_second_timeout() {
    let target = EvaluationTarget {
        profile_id: uuid::Uuid::new_v4().to_string(),
        model_id: "fixture".into(),
        reasoning_effort: None,
    };
    let mut legacy = serde_json::to_value(plan(&target)).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("requestTimeoutSeconds");
    let migrated: EvaluationPlan = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(migrated.request_timeout_seconds, 120);
    assert_eq!(EvaluationPlan::default().request_timeout_seconds, 300);
    let mut historical = serde_json::to_value(run(plan(&target))).unwrap();
    historical["plan"] = legacy.clone();
    let historical: EvaluationRun = serde_json::from_value(historical).unwrap();
    assert_eq!(historical.plan.request_timeout_seconds, 120);
    assert_eq!(
        serde_json::to_value(&historical).unwrap()["plan"]["requestTimeoutSeconds"],
        120
    );
    let index: storage::EvaluationStore = serde_json::from_value(json!({"plan":legacy})).unwrap();
    assert_eq!(index.plan.request_timeout_seconds, 120);
    assert_eq!(
        serde_json::to_value(&index).unwrap()["plan"]["requestTimeoutSeconds"],
        120
    );
    let empty: storage::EvaluationStore = serde_json::from_value(json!({})).unwrap();
    assert_eq!(empty.plan.request_timeout_seconds, 300);
    assert_eq!(
        storage::EvaluationStore::default()
            .plan
            .request_timeout_seconds,
        300
    );
    let mut customized = migrated;
    customized.request_timeout_seconds = 1800;
    let serialized = serde_json::to_value(&customized).unwrap();
    assert_eq!(serialized["requestTimeoutSeconds"], 1800);
    assert_eq!(
        serde_json::from_value::<EvaluationPlan>(serialized).unwrap(),
        customized
    );
}

#[test]
fn timeout_boundaries_are_enforced_for_saved_manual_and_scheduled_plans() {
    let target = EvaluationTarget {
        profile_id: uuid::Uuid::new_v4().to_string(),
        model_id: "fixture".into(),
        reasoning_effort: None,
    };
    for scheduled in [false, true] {
        let mut plan = plan(&target);
        plan.schedule_enabled = scheduled;
        for seconds in [30, 300, 3600] {
            plan.request_timeout_seconds = seconds;
            assert!(validate_shape(&plan, true).is_ok());
        }
        for seconds in [0, 29, 3601, u32::MAX] {
            plan.request_timeout_seconds = seconds;
            assert!(validate_shape(&plan, true).unwrap_err().contains("30–3600"));
        }
    }
}

#[tokio::test]
async fn configured_deadline_covers_headers_and_body_without_retries() {
    use std::time::{Duration, Instant};
    for mode in ["slow", "slow-body"] {
        let (url, calls, server, key) = fixture(mode).await;
        let target = target(url, key.clone());
        let mut plan = plan(&target.target);
        plan.cases = vec![CaseId::Candy];
        let (_sender, receiver) = watch::channel(false);
        let started = Instant::now();
        let result = runner::execute_with_timeout(
            PreparedPlan {
                targets: vec![target],
                judge: None,
            },
            run(plan),
            receiver,
            |_| Ok(()),
            Duration::from_millis(80),
        )
        .await;
        assert_eq!(result.results[0].status, CaseStatus::Error);
        let error = result.results[0]
            .error
            .as_ref()
            .expect("delayed fixture must time out");
        assert!(error.contains("超时"));
        assert!(error.contains("0.08 秒"), "{error}");
        assert!(!error.contains(&key));
        assert!((70..2000).contains(&result.results[0].elapsed_ms));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        server.abort();
    }
}

#[tokio::test]
async fn subject_and_judge_each_receive_a_fresh_full_deadline() {
    use std::time::{Duration, Instant};
    let (url, calls, server, key) = fixture("short-delays").await;
    let judge = target(url.clone(), key.clone());
    let subject = target(url, key);
    let mut plan = plan(&subject.target);
    plan.cases = vec![CaseId::Candy];
    plan.judge = Some(judge.target.clone());
    let (_sender, receiver) = watch::channel(false);
    let started = Instant::now();
    let result = runner::execute_with_timeout(
        PreparedPlan {
            targets: vec![subject],
            judge: Some(judge),
        },
        run(plan),
        receiver,
        |_| Ok(()),
        Duration::from_millis(350),
    )
    .await;
    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.results[0].score, Some(100));
    assert_eq!(result.results[0].judge.as_ref().unwrap().score, Some(92));
    assert!((200..350).contains(&result.results[0].elapsed_ms));
    assert!(started.elapsed() >= Duration::from_millis(400));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    server.abort();
}

#[tokio::test]
async fn an_independent_judge_uses_the_same_configured_timeout() {
    use std::time::Duration;
    let (url, calls, server, key) = fixture("slow-judge").await;
    let judge = target(url.clone(), key.clone());
    let subject = target(url, key);
    let mut plan = plan(&subject.target);
    plan.cases = vec![CaseId::Candy];
    plan.judge = Some(judge.target.clone());
    let (_sender, receiver) = watch::channel(false);
    let result = runner::execute_with_timeout(
        PreparedPlan {
            targets: vec![subject],
            judge: Some(judge),
        },
        run(plan),
        receiver,
        |_| Ok(()),
        Duration::from_millis(80),
    )
    .await;
    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.results[0].score, Some(100));
    let judge = result.results[0].judge.as_ref().unwrap();
    assert!(judge.score.is_none());
    assert!(judge.error.as_ref().unwrap().contains("0.08 秒"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    server.abort();
}

#[tokio::test]
async fn a_disconnected_server_is_reported_as_network_failure_not_timeout() {
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = target(
        format!("http://{}", listener.local_addr().unwrap()),
        "fixture-only-key".into(),
    );
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 1024];
        let _ = socket.read(&mut buffer).await;
    });
    let (_sender, receiver) = watch::channel(false);
    let error = client::request(
        &client::http_client().unwrap(),
        &target,
        "test",
        Duration::from_secs(30),
        receiver,
    )
    .await
    .err()
    .unwrap();
    assert!(error.message.contains("网络连接中断"));
    assert!(!error.message.contains("超时"));
    server.await.unwrap();
}

#[tokio::test]
async fn pelican_preserves_large_animated_html_without_scoring_or_judge_requests() {
    let (url, calls, server, key) = fixture("animation").await;
    let subject = target(url.clone(), key.clone());
    let judge = target(url, key);
    let mut selected = plan(&subject.target);
    selected.cases = vec![CaseId::Pelican];
    selected.judge = Some(judge.target.clone());
    assert!(judge_target(&selected).is_none());
    let (_sender, receiver) = watch::channel(false);
    let completed = runner::execute(
        PreparedPlan {
            targets: vec![subject],
            judge: Some(judge),
        },
        run(selected),
        receiver,
        |_| Ok(()),
    )
    .await;
    assert_eq!(completed.status, RunStatus::Completed);
    assert_eq!(completed.completed_cases, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let result = &completed.results[0];
    assert_eq!(result.status, CaseStatus::Generated);
    assert_eq!(serde_json::to_value(result.status).unwrap(), "generated");
    assert!(result.score.is_none());
    assert_eq!(result.max_score, 0);
    assert!(result.checks.is_empty());
    assert!(result.judge.is_none());
    assert!(result.safe_svg.is_none());
    let html = result.artifact_html.as_ref().unwrap();
    assert!(html.len() > client::MAX_OUTPUT_BYTES);
    assert!(html.contains("<animate"));
    assert!(html.contains("requestAnimationFrame"));
    assert!(!html.contains("```"));
    server.abort();
}

#[tokio::test]
async fn pelican_output_remains_bounded_and_is_never_retried() {
    let (url, calls, server, key) = fixture("animation-large").await;
    let subject = target(url, key);
    let mut selected = plan(&subject.target);
    selected.cases = vec![CaseId::Pelican];
    let (_sender, receiver) = watch::channel(false);
    let completed = runner::execute(
        PreparedPlan {
            targets: vec![subject],
            judge: None,
        },
        run(selected),
        receiver,
        |_| Ok(()),
    )
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(completed.results[0].status, CaseStatus::Error);
    assert!(completed.results[0].artifact_html.is_none());
    assert!(completed.results[0]
        .error
        .as_ref()
        .unwrap()
        .contains("输出上限"));
    server.abort();
}

#[test]
fn minute_schedules_preserve_legacy_hours_and_validate_the_effective_interval() {
    let default = EvaluationPlan::default();
    assert_eq!(default.interval_minutes, Some(30));
    let mut legacy = serde_json::to_value(&default).unwrap();
    legacy.as_object_mut().unwrap().remove("intervalMinutes");
    legacy["intervalHours"] = json!(6);
    let mut migrated: EvaluationPlan = serde_json::from_value(legacy).unwrap();
    assert_eq!(migrated.interval_minutes, None);
    assert_eq!(migrated.effective_interval_minutes(), 360);
    assert!(validate_shape(&migrated, false).is_ok());
    let now = Utc::now();
    assert_eq!(
        scheduler::next_time(now, migrated.effective_interval_minutes()),
        (now + chrono::Duration::hours(6)).to_rfc3339()
    );
    for minutes in [10, 30, 60, 1440, 10080] {
        migrated.interval_minutes = Some(minutes);
        assert!(validate_shape(&migrated, false).is_ok());
    }
    for minutes in [0, 9, 10081, u32::MAX] {
        migrated.interval_minutes = Some(minutes);
        assert!(validate_shape(&migrated, false)
            .unwrap_err()
            .contains("10–10080"));
    }
    migrated.interval_minutes = None;
    migrated.interval_hours = u32::MAX;
    assert!(validate_shape(&migrated, false).is_err());
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut store = storage::read(&paths).unwrap();
    store.plan.schedule_enabled = true;
    storage::write(&paths, &store).unwrap();
    assert!(scheduler::claim(&paths, now).unwrap().is_some());
    assert_eq!(
        storage::read(&paths).unwrap().next_run_at,
        Some((now + chrono::Duration::minutes(30)).to_rfc3339())
    );
}

#[test]
fn legacy_activity_migrates_once_and_polls_without_loading_full_reports() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let target = EvaluationTarget {
        profile_id: uuid::Uuid::new_v4().to_string(),
        model_id: "fixture-model".into(),
        reasoning_effort: Some("high".into()),
    };
    let mut historical = run(plan(&target));
    historical.status = RunStatus::Completed;
    historical.finished_at = Some(Utc::now().to_rfc3339());
    let mut legacy_result = result(&target, CaseId::Pelican);
    legacy_result.artifact_html = None;
    legacy_result.safe_svg = Some("<svg viewBox=\"0 0 10 10\"></svg>".into());
    legacy_result.status = CaseStatus::Passed;
    legacy_result.score = Some(100);
    historical.results.push(legacy_result);
    let mut serialized = serde_json::to_value(&historical).unwrap();
    serialized["results"][0]
        .as_object_mut()
        .unwrap()
        .remove("artifactHtml");
    let run_path = paths
        .data
        .join("evaluations/runs")
        .join(format!("{}.json", historical.id));
    core::atomic_write(&run_path, &serde_json::to_vec_pretty(&serialized).unwrap()).unwrap();
    let legacy_index = json!({
        "plan": historical.plan,
        "history": [RunSummary::from(&historical)],
        "activeRunId": null
    });
    let index_path = paths.data.join("evaluations/index.json");
    core::atomic_write(
        &index_path,
        &serde_json::to_vec_pretty(&legacy_index).unwrap(),
    )
    .unwrap();
    let state = EvaluationState::new(paths.clone());
    let activity = state.activity().unwrap();
    assert_eq!(activity.records.len(), 1);
    let record = &activity.records[0];
    assert!(record.has_artifact);
    assert_eq!(record.score, Some(100));
    assert_eq!(
        record.id,
        json!([historical.id, target.profile_id, target.model_id, "pelican"]).to_string()
    );
    let compact = serde_json::to_string(&activity).unwrap();
    for excluded in ["prompt", "output", "<svg", "artifactHtml", "safeSvg"] {
        assert!(!compact.contains(excluded));
    }
    let index_before = std::fs::read(&index_path).unwrap();
    assert_eq!(storage::read(&paths).unwrap().records_version, 1);
    assert!(state.run(&historical.id).unwrap().results[0]
        .artifact_html
        .is_none());
    core::atomic_write(&run_path, b"deliberately unreadable report after migration").unwrap();
    let restarted = EvaluationState::new(paths.clone());
    assert!(restarted.dashboard().unwrap().error.is_none());
    assert_eq!(
        serde_json::to_string(&restarted.activity().unwrap()).unwrap(),
        compact
    );
    assert_eq!(std::fs::read(&index_path).unwrap(), index_before);
    assert!(!paths.config.exists());
}

#[test]
fn finished_activity_is_compact_idempotent_and_tracks_the_saved_artifact() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let target = EvaluationTarget {
        profile_id: uuid::Uuid::new_v4().to_string(),
        model_id: "fixture-model".into(),
        reasoning_effort: None,
    };
    let mut completed = run(plan(&target));
    completed.status = RunStatus::Completed;
    completed.results.push(result(&target, CaseId::Pelican));
    for _ in 0..2 {
        storage::finish(&paths, &completed).unwrap();
    }
    let state = EvaluationState::new(paths.clone());
    let activity = state.activity().unwrap();
    assert_eq!(activity.records.len(), 1);
    assert_eq!(activity.records[0].status, CaseStatus::Generated);
    assert!(activity.records[0].has_artifact);
    assert!(activity.records[0].score.is_none());
    let full = state.run(&completed.id).unwrap();
    assert_eq!(
        full.results[0].artifact_html,
        completed.results[0].artifact_html
    );
    let export: serde_json::Value =
        serde_json::from_str(&state.export(&completed.id).unwrap().content).unwrap();
    assert_eq!(
        export["results"][0]["artifactHtml"],
        completed.results[0].artifact_html.as_deref().unwrap()
    );
    assert!(
        !std::fs::read_to_string(paths.data.join("evaluations/index.json"))
            .unwrap()
            .contains("fixture output")
    );
}
