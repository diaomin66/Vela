import { invoke } from '@tauri-apps/api/core';
import { api, desktop } from './api';
import { createPreviewEvaluation } from './evaluation/preview';
import type { EvaluationApi, EvaluationPlan, EvaluationTarget } from './evaluation/types';
export type * from './evaluation/types';

const native: EvaluationApi = {
  dashboard: () => invoke('get_evaluation_dashboard'),
  activity: () => invoke('get_evaluation_activity'),
  save: (plan) => invoke('save_evaluation_plan', { plan }),
  start: (plan) => invoke('start_evaluation', { plan }),
  cancel: (runId) => invoke('cancel_evaluation', { runId }),
  remove: (runIds) => invoke('delete_evaluation_runs', { runIds }),
  run: (runId) => invoke('get_evaluation_run', { runId }),
  export: (runId) => invoke('export_evaluation_run', { runId }),
};
export const evaluationApi = desktop ? native : createPreviewEvaluation(Date.now, () => api.dashboard(), typeof window !== 'undefined' && new URLSearchParams(window.location.search).get('evaluationDemo') === 'gallery');
export const targetKey = (target: Pick<EvaluationTarget, 'profileId' | 'modelId'>) => JSON.stringify([target.profileId, target.modelId]);
export const requestBudget = (plan: EvaluationPlan) => plan.targets.length * (plan.cases.length + (plan.judge ? plan.cases.filter((id) => id !== 'pelican').length : 0));
export const intervalMinutes = (plan: EvaluationPlan) => plan.intervalMinutes ?? plan.intervalHours * 60;
export function validateEvaluationPlan(plan: EvaluationPlan): string | null {
  if (!plan.targets.length) return '请至少选择一个模型。';
  if (plan.targets.length > 6) return '每轮最多选择 6 个模型。';
  if (new Set(plan.targets.map(targetKey)).size !== plan.targets.length) return '同一渠道的模型不能重复选择。';
  if (!plan.cases.length) return '请至少选择一项测试。';
  if (plan.targets.some((target) => target.reasoningEffort === 'ultra') || plan.judge?.reasoningEffort === 'ultra') return '评测直接调用模型 API，请选择普通推理档位；Ultra 请在原生任务中使用。';
  const minutes = intervalMinutes(plan);
  if (!Number.isInteger(minutes) || minutes < 10 || minutes > 10080) return '定时间隔应为 10 分钟–7 天。';
  if (!Number.isInteger(plan.requestTimeoutSeconds) || plan.requestTimeoutSeconds < 30 || plan.requestTimeoutSeconds > 3600) return '请求超时应为 30–3600 秒的整数。';
  return null;
}
