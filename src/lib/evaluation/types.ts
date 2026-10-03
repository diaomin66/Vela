export type EvaluationCaseId = 'candy' | 'pelican' | 'judgment';
export interface EvaluationTarget { profileId: string; modelId: string; reasoningEffort: string | null }
export interface EvaluationPlan { targets: EvaluationTarget[]; cases: EvaluationCaseId[]; judge: EvaluationTarget | null; scheduleEnabled: boolean; intervalHours: number }
export interface EvaluationCase { id: EvaluationCaseId; title: string; description: string; version: string }
export interface EvaluationCheck { label: string; passed: boolean }
export interface JudgeResult { score: number | null; explanation: string; profileId: string; modelId: string; error: string | null }
export interface CaseResult {
  profileId: string; channelName: string; modelId: string; modelAlias: string; reasoningEffort: string | null;
  caseId: EvaluationCaseId; status: 'passed' | 'failed' | 'error' | 'cancelled'; score: number | null; maxScore: number;
  checks: EvaluationCheck[]; prompt: string; output: string; safeSvg: string | null; elapsedMs: number;
  inputTokens: number | null; outputTokens: number | null; error: string | null; judge: JudgeResult | null;
}
export interface RunSummary {
  id: string; trigger: 'manual' | 'scheduled'; status: 'running' | 'completed' | 'cancelled' | 'interrupted';
  startedAt: string; finishedAt: string | null; completedCases: number; totalCases: number; targetCount: number;
}
export interface EvaluationRun extends RunSummary { caseVersion: string; plan: EvaluationPlan; results: CaseResult[]; error: string | null }
export interface EvaluationDashboard { plan: EvaluationPlan; cases: EvaluationCase[]; active: EvaluationRun | null; history: RunSummary[]; nextRunAt: string | null; error: string | null }
export interface EvaluationExport { fileName: string; content: string; path: string }
export interface EvaluationApi {
  dashboard(): Promise<EvaluationDashboard>;
  save(plan: EvaluationPlan): Promise<EvaluationDashboard>;
  start(plan: EvaluationPlan): Promise<EvaluationDashboard>;
  cancel(runId: string): Promise<EvaluationDashboard>;
  run(runId: string): Promise<EvaluationRun>;
  export(runId: string): Promise<EvaluationExport>;
}
