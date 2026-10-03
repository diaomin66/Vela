//! Serializable evaluation contracts. No credentials or raw transport diagnostics.
use serde::{Deserialize, Serialize};

pub const DEFAULT_REQUEST_TIMEOUT_SECONDS: u32 = 300;
fn legacy_request_timeout_seconds() -> u32 {
    120
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationTarget {
    pub profile_id: String,
    pub model_id: String,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum CaseId {
    Candy,
    Pelican,
    Judgment,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationPlan {
    pub targets: Vec<EvaluationTarget>,
    pub cases: Vec<CaseId>,
    #[serde(default)]
    pub judge: Option<EvaluationTarget>,
    pub schedule_enabled: bool,
    pub interval_hours: u32,
    #[serde(default)]
    pub interval_minutes: Option<u32>,
    #[serde(default = "legacy_request_timeout_seconds")]
    pub request_timeout_seconds: u32,
}

impl Default for EvaluationPlan {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            cases: vec![CaseId::Candy, CaseId::Pelican],
            judge: None,
            schedule_enabled: false,
            interval_hours: 24,
            interval_minutes: Some(30),
            request_timeout_seconds: DEFAULT_REQUEST_TIMEOUT_SECONDS,
        }
    }
}

impl EvaluationPlan {
    pub fn effective_interval_minutes(&self) -> u32 {
        self.interval_minutes
            .unwrap_or_else(|| self.interval_hours.saturating_mul(60))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseDefinition {
    pub id: CaseId,
    pub title: String,
    pub description: String,
    pub version: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RunTrigger {
    Manual,
    Scheduled,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RunStatus {
    Running,
    Completed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CaseStatus {
    Generated,
    Passed,
    Failed,
    Error,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationCheck {
    pub label: String,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JudgeResult {
    pub score: Option<u32>,
    pub explanation: String,
    pub profile_id: String,
    pub model_id: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseResult {
    pub profile_id: String,
    pub channel_name: String,
    pub model_id: String,
    pub model_alias: String,
    pub reasoning_effort: Option<String>,
    pub case_id: CaseId,
    pub status: CaseStatus,
    pub score: Option<u32>,
    pub max_score: u32,
    pub checks: Vec<EvaluationCheck>,
    pub prompt: String,
    pub output: String,
    pub safe_svg: Option<String>,
    #[serde(default)]
    pub artifact_html: Option<String>,
    pub elapsed_ms: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub error: Option<String>,
    pub judge: Option<JudgeResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationRecord {
    pub id: String,
    pub run_id: String,
    pub created_at: String,
    pub trigger: RunTrigger,
    pub case_version: String,
    pub profile_id: String,
    pub channel_name: String,
    pub model_id: String,
    pub model_alias: String,
    pub reasoning_effort: Option<String>,
    pub case_id: CaseId,
    pub status: CaseStatus,
    pub score: Option<u32>,
    pub elapsed_ms: u64,
    pub has_artifact: bool,
}

impl EvaluationRecord {
    pub fn from_result(run: &EvaluationRun, result: &CaseResult) -> Self {
        Self {
            id: serde_json::json!([run.id, result.profile_id, result.model_id, result.case_id])
                .to_string(),
            run_id: run.id.clone(),
            created_at: run.started_at.clone(),
            trigger: run.trigger,
            case_version: run.case_version.clone(),
            profile_id: result.profile_id.clone(),
            channel_name: result.channel_name.clone(),
            model_id: result.model_id.clone(),
            model_alias: result.model_alias.clone(),
            reasoning_effort: result.reasoning_effort.clone(),
            case_id: result.case_id,
            status: result.status,
            score: result.score,
            elapsed_ms: result.elapsed_ms,
            has_artifact: result.artifact_html.is_some() || result.safe_svg.is_some(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationActivity {
    pub records: Vec<EvaluationRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationRun {
    pub id: String,
    pub trigger: RunTrigger,
    pub status: RunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub case_version: String,
    pub completed_cases: usize,
    pub total_cases: usize,
    pub target_count: usize,
    pub plan: EvaluationPlan,
    pub results: Vec<CaseResult>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub id: String,
    pub trigger: RunTrigger,
    pub status: RunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub completed_cases: usize,
    pub total_cases: usize,
    pub target_count: usize,
}

impl From<&EvaluationRun> for RunSummary {
    fn from(run: &EvaluationRun) -> Self {
        Self {
            id: run.id.clone(),
            trigger: run.trigger,
            status: run.status,
            started_at: run.started_at.clone(),
            finished_at: run.finished_at.clone(),
            completed_cases: run.completed_cases,
            total_cases: run.total_cases,
            target_count: run.target_count,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationDashboard {
    pub plan: EvaluationPlan,
    pub cases: Vec<CaseDefinition>,
    pub active: Option<EvaluationRun>,
    pub history: Vec<RunSummary>,
    pub next_run_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationExport {
    pub file_name: String,
    pub content: String,
    pub path: String,
}
