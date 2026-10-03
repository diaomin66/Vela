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
    } else if prompt.starts_with("你是评审") {
        r#"{"score":92,"explanation":"This is a subjective text review."}"#.into()
    } else if prompt.contains("J1") {
        r#"{"J1":true,"J2":false,"J3":false,"J4":true,"J5":false,"J6":true}"#.into()
    } else {
        r#"{"answer":21}"#.into()
    };
    Json(json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":text}]}],"usage":{"input_tokens":30,"output_tokens":8}})).into_response()
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
        storage::finish(&paths, &next).unwrap();
    }
    assert_eq!(storage::read(&paths).unwrap().history.len(), 100);
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
