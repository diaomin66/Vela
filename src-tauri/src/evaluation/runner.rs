//! Serial execution: each task and optional text review is sent at most once.
use super::{
    artifact, cases,
    client::{self, PreparedTarget},
    types::*,
};
use chrono::Utc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

pub(super) struct PreparedPlan {
    pub targets: Vec<PreparedTarget>,
    pub judge: Option<PreparedTarget>,
}

pub(super) async fn execute(
    prepared: PreparedPlan,
    run: EvaluationRun,
    cancel: watch::Receiver<bool>,
    progress: impl Fn(&EvaluationRun) -> Result<(), String>,
) -> EvaluationRun {
    let timeout = Duration::from_secs(u64::from(run.plan.request_timeout_seconds));
    execute_with_timeout(prepared, run, cancel, progress, timeout).await
}

pub(super) async fn execute_with_timeout(
    prepared: PreparedPlan,
    mut run: EvaluationRun,
    cancel: watch::Receiver<bool>,
    progress: impl Fn(&EvaluationRun) -> Result<(), String>,
    timeout: Duration,
) -> EvaluationRun {
    let client = match client::http_client() {
        Ok(client) => client,
        Err(error) => {
            run.status = RunStatus::Interrupted;
            run.error = Some(error);
            run.finished_at = Some(Utc::now().to_rfc3339());
            return run;
        }
    };
    'models: for target in &prepared.targets {
        for case_id in run.plan.cases.clone() {
            if *cancel.borrow() {
                run.status = RunStatus::Cancelled;
                break 'models;
            }
            let prompt = cases::prompt(case_id);
            let mut result = CaseResult {
                profile_id: target.target.profile_id.clone(),
                channel_name: target.channel_name.clone(),
                model_id: target.target.model_id.clone(),
                model_alias: target.model_alias.clone(),
                reasoning_effort: target.target.reasoning_effort.clone(),
                case_id,
                status: CaseStatus::Error,
                score: None,
                max_score: if case_id == CaseId::Pelican { 0 } else { 100 },
                checks: Vec::new(),
                prompt: prompt.into(),
                output: String::new(),
                safe_svg: None,
                artifact_html: None,
                elapsed_ms: 0,
                input_tokens: None,
                output_tokens: None,
                error: None,
                judge: None,
            };
            let started = Instant::now();
            let answer = if case_id == CaseId::Pelican {
                client::request_artifact(&client, target, prompt, timeout, cancel.clone()).await
            } else {
                client::request(&client, target, prompt, timeout, cancel.clone()).await
            };
            match answer {
                Ok(answer) => {
                    if let Some(grade) = cases::grade(case_id, &answer.text) {
                        result.status = if grade.score == 100 {
                            CaseStatus::Passed
                        } else {
                            CaseStatus::Failed
                        };
                        result.score = Some(grade.score);
                        result.checks = grade.checks;
                    } else {
                        result.status = CaseStatus::Generated;
                        result.artifact_html = artifact::extract_html(&answer.text);
                    }
                    result.output = answer.text;
                    result.elapsed_ms = answer.elapsed_ms;
                    result.input_tokens = answer.input_tokens;
                    result.output_tokens = answer.output_tokens;
                    if let Some(judge) = prepared
                        .judge
                        .as_ref()
                        .filter(|_| case_id != CaseId::Pelican)
                    {
                        let mut review = JudgeResult {
                            score: None,
                            explanation: String::new(),
                            profile_id: judge.target.profile_id.clone(),
                            model_id: judge.target.model_id.clone(),
                            error: None,
                        };
                        match client::request(
                            &client,
                            judge,
                            &cases::judge_prompt(case_id, &result.output),
                            timeout,
                            cancel.clone(),
                        )
                        .await
                        {
                            Ok(answer) => {
                                let value = cases::json_answer(&answer.text);
                                let score = value
                                    .as_ref()
                                    .and_then(|v| v.get("score"))
                                    .and_then(serde_json::Value::as_u64)
                                    .filter(|score| *score <= 100);
                                let explanation = value
                                    .as_ref()
                                    .and_then(|v| v.get("explanation"))
                                    .and_then(serde_json::Value::as_str);
                                if let (Some(score), Some(explanation)) = (score, explanation) {
                                    review.score = Some(score as u32);
                                    review.explanation =
                                        client::redact(explanation, &judge.key, 2400);
                                } else {
                                    review.error =
                                        Some("评审未返回有效的评分 JSON，未自动重试。".into());
                                }
                            }
                            Err(error) => {
                                review.error = Some(error.message);
                                if error.cancelled {
                                    run.status = RunStatus::Cancelled;
                                }
                            }
                        }
                        result.judge = Some(review);
                    }
                }
                Err(error) => {
                    result.elapsed_ms = started.elapsed().as_millis() as u64;
                    result.status = if error.cancelled {
                        CaseStatus::Cancelled
                    } else {
                        CaseStatus::Error
                    };
                    result.error = Some(error.message);
                    if error.cancelled {
                        run.status = RunStatus::Cancelled;
                    }
                }
            }
            if result.status != CaseStatus::Cancelled {
                run.completed_cases += 1;
            }
            run.results.push(result);
            if let Err(error) = progress(&run) {
                run.status = RunStatus::Interrupted;
                run.error = Some(error);
                break 'models;
            }
            if run.status == RunStatus::Cancelled {
                break 'models;
            }
        }
    }
    if run.status == RunStatus::Running {
        run.status = if *cancel.borrow() {
            RunStatus::Cancelled
        } else {
            RunStatus::Completed
        };
    }
    run.finished_at = Some(Utc::now().to_rfc3339());
    run
}
