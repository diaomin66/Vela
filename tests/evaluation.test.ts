import { afterEach, describe, expect, it, vi } from 'vitest';
import { createEvaluationStore } from '../src/hooks/useEvaluations';
import { requestBudget, targetKey, validateEvaluationPlan } from '../src/lib/evaluation';
import { createPreviewEvaluation } from '../src/lib/evaluation/preview';
import type { EvaluationApi, EvaluationDashboard, EvaluationPlan } from '../src/lib/evaluation/types';

const plan = (): EvaluationPlan => ({ targets: [{ profileId: 'a', modelId: 'shared', reasoningEffort: 'high' }], cases: ['candy', 'pelican', 'judgment'], judge: null, scheduleEnabled: false, intervalHours: 24, requestTimeoutSeconds: 300 });
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe('evaluation plan boundaries', () => {
  it('counts both generation and optional review calls without conflating channels', () => {
    const value = plan();
    value.targets.push({ profileId: 'b', modelId: 'shared', reasoningEffort: null });
    expect(targetKey(value.targets[0])).not.toBe(targetKey(value.targets[1]));
    expect(validateEvaluationPlan(value)).toBeNull();
    expect(requestBudget(value)).toBe(6);
    value.judge = { profileId: 'c', modelId: 'reviewer', reasoningEffort: 'medium' };
    expect(requestBudget(value)).toBe(12);
  });
  it('rejects duplicate targets, native Ultra and invalid schedules before calling the backend', () => {
    const value = plan(); value.targets.push({ ...value.targets[0] });
    expect(validateEvaluationPlan(value)).toMatch('重复');
    value.targets.pop(); value.targets[0].reasoningEffort = 'ultra';
    expect(validateEvaluationPlan(value)).toMatch('Ultra');
    value.targets[0].reasoningEffort = null; value.intervalHours = 0;
    expect(validateEvaluationPlan(value)).toMatch('1–168');
  });
  it('accepts custom whole-second request timeouts within the supported limits', () => {
    for (const requestTimeoutSeconds of [30, 125, 300, 3600]) expect(validateEvaluationPlan({ ...plan(), requestTimeoutSeconds })).toBeNull();
    for (const requestTimeoutSeconds of [0, 29, 3601, 30.5, NaN, Infinity]) expect(validateEvaluationPlan({ ...plan(), requestTimeoutSeconds })).toMatch('30–3600');
  });
});

describe('evaluation demo isolation', () => {
  it('keeps progress, cancellation and history without real requests or automatic scheduling', async () => {
    let now = 0;
    const fetch = vi.fn(() => { throw new Error('No evaluation demo network'); }); vi.stubGlobal('fetch', fetch);
    const api = createPreviewEvaluation(() => now);
    const saved = plan(); saved.scheduleEnabled = true;
    await api.save(saved); now += 48 * 3600000;
    expect((await api.dashboard()).history).toHaveLength(0);
    const run = (await api.start(plan())).active!;
    await expect(api.start(plan())).rejects.toThrow('正在运行');
    now += 700;
    expect((await api.dashboard()).active!.completedCases).toBe(1);
    const cancelled = await api.cancel(run.id);
    expect(cancelled.active).toBeNull();
    expect(cancelled.history[0].status).toBe('cancelled');
    now += 10000;
    expect((await api.run(run.id)).results).toHaveLength(1);
    expect((await api.dashboard()).plan.scheduleEnabled).toBe(true);
    expect(fetch).not.toHaveBeenCalled();
  });
  it('snapshots each run and exports full answers without changing the saved schedule', async () => {
    let now = 0;
    const api = createPreviewEvaluation(() => now);
    const input = plan();
    const id = (await api.start(input)).active!.id;
    input.cases.splice(0); now += 3000;
    const result = await api.run(id);
    expect(result.status).toBe('completed');
    expect(result.plan.cases).toHaveLength(3);
    expect(result.results.find((item) => item.caseId === 'pelican')!.safeSvg).toContain('<svg');
    expect(JSON.parse((await api.export(id)).content).results).toHaveLength(3);
    expect((await api.dashboard()).plan.targets).toHaveLength(0);
  });
  it('defaults to five minutes and snapshots custom timeouts independently from the scheduled plan', async () => {
    let now = 0;
    const api = createPreviewEvaluation(() => now);
    expect((await api.dashboard()).plan.requestTimeoutSeconds).toBe(300);
    const saved = { ...plan(), scheduleEnabled: true, requestTimeoutSeconds: 900 };
    await api.save(saved);
    const input = { ...saved, requestTimeoutSeconds: 125 };
    const id = (await api.start(input)).active!.id;
    input.requestTimeoutSeconds = 60;
    await api.save({ ...saved, requestTimeoutSeconds: 1800 });
    now += 3000;
    expect((await api.run(id)).plan.requestTimeoutSeconds).toBe(125);
    expect(JSON.parse((await api.export(id)).content).plan.requestTimeoutSeconds).toBe(125);
    expect((await api.dashboard()).plan.requestTimeoutSeconds).toBe(1800);
    expect((await api.dashboard()).plan.scheduleEnabled).toBe(true);
  });
});

describe('evaluation polling and command ordering', () => {
  it('ignores an older poll after starting a run and shares a single polling lifecycle', async () => {
    vi.useFakeTimers(); vi.stubGlobal('document', Object.assign(new EventTarget(), { hidden: false }));
    const preview = createPreviewEvaluation(); const initial = await preview.dashboard();
    let finishRead!: (value: EvaluationDashboard) => void;
    const running = await preview.start(plan());
    const api: EvaluationApi = { ...preview, dashboard: vi.fn(() => new Promise<EvaluationDashboard>((resolve) => { finishRead = resolve; })), start: vi.fn(async () => running) };
    const store = createEvaluationStore(api);
    const stopA = store.subscribe(() => {}); const stopB = store.subscribe(() => {});
    expect(api.dashboard).toHaveBeenCalledTimes(1);
    await store.start(plan()); finishRead(initial); await Promise.resolve();
    expect(store.getSnapshot().data!.active!.id).toBe(running.active!.id);
    stopA(); stopB(); await vi.advanceTimersByTimeAsync(30000);
    expect(api.dashboard).toHaveBeenCalledTimes(1);
  });
  it('prevents duplicate starts while the first command is in flight and preserves state after failure', async () => {
    const preview = createPreviewEvaluation();
    let finishStart!: (value: EvaluationDashboard) => void;
    const api: EvaluationApi = { ...preview, start: vi.fn(() => new Promise<EvaluationDashboard>((resolve) => { finishStart = resolve; })), save: vi.fn(async () => { throw new Error('Plan could not be saved'); }) };
    const store = createEvaluationStore(api);
    const first = store.start(plan());
    expect(await store.start(plan())).toBe(false);
    expect(api.start).toHaveBeenCalledTimes(1);
    const running = await preview.start(plan()); finishStart(running); await first;
    expect(await store.save(plan())).toBe(false);
    expect(store.getSnapshot().data!.active!.id).toBe(running.active!.id);
    expect(store.getSnapshot().error).toBe('Plan could not be saved');
  });
});
