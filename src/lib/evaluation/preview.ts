import type { Dashboard } from '../../types';
import type { CaseResult, EvaluationApi, EvaluationDashboard, EvaluationPlan, EvaluationRun, RunSummary } from './types';

const pelican = '<svg xmlns="http://www.w3.org/2000/svg" width="480" height="300" viewBox="0 0 480 300"><rect width="480" height="300" rx="28" fill="#edf2e7"/><path d="M56 246h368" stroke="#b8c5a9" stroke-width="2"/><g fill="none" stroke="#52735a" stroke-width="5"><circle cx="149" cy="208" r="37"/><circle cx="334" cy="208" r="37"/><path d="m149 208 58-78 55 78H149l87-69 26 69 44-107m-21 0h37m-141 28h42"/></g><path d="M156 131c-38-48 21-85 75-50l43 7 11 42-29 20-66-2z" fill="#fff"/><path d="M166 102c25-8 48 1 60 30-23 3-42-7-60-30" fill="#c3d2b6"/><path d="M235 90c-14-51 49-65 58-32l2 42-27 23" fill="#fff"/><path d="m291 62 75 16-73 17" fill="#dca557"/><path d="M295 81q40 30 69-2" fill="#e7bb79"/><circle cx="280" cy="58" r="4" fill="#294a35"/><path d="m233 139 17 36 24 7m-66-41 16 34 23 11" fill="none" stroke="#a1804e" stroke-width="5" stroke-linecap="round"/></svg>';

// All results are explicitly simulated. This adapter has no network, filesystem,
// real scheduler or credential access, and survives page navigation in memory.
export function createPreviewEvaluation(now: () => number = Date.now, workspace?: () => Promise<Dashboard>): EvaluationApi {
  let saved: EvaluationPlan = { targets: [], cases: ['candy', 'pelican'], judge: null, scheduleEnabled: false, intervalHours: 24, requestTimeoutSeconds: 300 };
  let active: EvaluationRun | null = null;
  let nextRunAt: string | null = null;
  const runs: EvaluationRun[] = [];
  let plannedResults: CaseResult[] = [];
  const stamp = () => new Date(now()).toISOString();
  const summary = ({ id, trigger, status, startedAt, finishedAt, completedCases, totalCases, targetCount }: EvaluationRun): RunSummary => ({ id, trigger, status, startedAt, finishedAt, completedCases, totalCases, targetCount });
  function advance() {
    if (!active) return;
    const count = Math.min(active.totalCases, Math.floor((now() - Date.parse(active.startedAt)) / 650));
    active.results = plannedResults.slice(0, count);
    active.completedCases = count;
    if (count === active.totalCases) {
      active.status = 'completed'; active.finishedAt = stamp(); runs.unshift(active); runs.splice(100); active = null;
    }
  }
  function dashboard(): EvaluationDashboard {
    advance();
    return structuredClone({ plan: saved, cases: [
      { id: 'candy', title: '糖果推理', description: '固定抽取规则，核对最小保证数量。', version: 'vela-demo-1' },
      { id: 'pelican', title: '鹈鹕绘图', description: '绘制骑自行车的鹈鹕，检查 SVG 结构。', version: 'vela-demo-1' },
      { id: 'judgment', title: '模型判题', description: '识别正确与错误答案，核对判断标签。', version: 'vela-demo-1' },
    ], active, history: runs.map(summary), nextRunAt, error: null });
  }
  function find(id: string) {
    advance(); const run = active?.id === id ? active : runs.find((item) => item.id === id);
    if (!run) throw new Error('这条评测记录已经不存在。');
    return structuredClone(run);
  }
  return {
    async dashboard() { return dashboard(); },
    async save(plan) { saved = structuredClone(plan); nextRunAt = plan.scheduleEnabled ? new Date(now() + plan.intervalHours * 3600000).toISOString() : null; return dashboard(); },
    async start(plan) {
      advance();
      if (active) throw new Error('已有评测正在运行。');
      if (!plan.targets.length || !plan.cases.length) throw new Error('请选择模型与测试项目。');
      const data = await workspace?.();
      plannedResults = plan.targets.flatMap((target, index) => plan.cases.map((caseId): CaseResult => {
        const profile = data?.profiles.find((item) => item.id === target.profileId);
        const svg = caseId === 'pelican';
        return { ...target, channelName: profile?.name ?? '演示渠道', modelAlias: profile?.models.find((m) => m.id === target.modelId)?.alias ?? '', caseId,
          status: index === 1 && caseId === 'candy' ? 'failed' : 'passed', score: index === 1 && caseId === 'candy' ? 0 : 100, maxScore: 100,
          checks: [{ label: svg ? 'SVG 格式与结构检查' : '答案与本机参考结果一致', passed: !(index === 1 && caseId === 'candy') }],
          prompt: svg ? 'Draw a pelican riding a bicycle as an SVG.' : '演示题目；桌面版会显示本次实际发送的完整题目。',
          output: svg ? pelican : caseId === 'candy' ? index === 1 ? '{"answer":20}' : '{"answer":21}' : '{"J1":true,"J2":false,"J3":false,"J4":true,"J5":false,"J6":true}',
          safeSvg: svg ? pelican : null, elapsedMs: 2400 + index * 640 + (svg ? 2200 : 0), inputTokens: 184, outputTokens: svg ? 740 : 124,
          error: null, judge: plan.judge ? { score: 88 - index * 7, explanation: svg ? '演示复评：依据 SVG 文本检查元素与布局，不是视觉评分。' : '演示复评：答案清晰，推导还可以展开。', profileId: plan.judge.profileId, modelId: plan.judge.modelId, error: null } : null,
        };
      }));
      active = { id: crypto.randomUUID(), trigger: 'manual', status: 'running', startedAt: stamp(), finishedAt: null, caseVersion: 'vela-demo-1', completedCases: 0, totalCases: plannedResults.length, targetCount: plan.targets.length, plan: structuredClone(plan), results: [], error: null };
      return dashboard();
    },
    async cancel(runId) { advance(); if (active?.id === runId) { active.status = 'cancelled'; active.finishedAt = stamp(); runs.unshift(active); active = null; } return dashboard(); },
    async run(runId) { return find(runId); },
    async export(runId) { return { fileName: `vela-evaluation-${runId}.json`, content: JSON.stringify(find(runId), null, 2), path: '' }; },
  };
}
