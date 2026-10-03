import { afterEach, describe, expect, it, vi } from 'vitest';
import { requestBudget, targetKey, validateEvaluationPlan } from '../src/lib/evaluation';
import { createPreviewEvaluation } from '../src/lib/evaluation/preview';
import type { EvaluationPlan } from '../src/lib/evaluation/types';

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
    expect(requestBudget(value)).toBe(10);
  });
  it('rejects duplicate targets, native Ultra and invalid schedules before calling the backend', () => {
    const value = plan(); value.targets.push({ ...value.targets[0] });
    expect(validateEvaluationPlan(value)).toMatch('重复');
    value.targets.pop(); value.targets[0].reasoningEffort = 'ultra';
    expect(validateEvaluationPlan(value)).toMatch('Ultra');
    value.targets[0].reasoningEffort = null; value.intervalHours = 0;
    expect(validateEvaluationPlan(value)).toMatch('10 分钟–7 天');
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
    const artwork = result.results.find((item) => item.caseId === 'pelican')!;
    expect(artwork.artifactHtml).toContain('<svg');
    expect(artwork.status).toBe('generated');
    expect(artwork.score).toBeNull();
    expect(artwork.checks).toEqual([]);
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

describe('activity metadata and compatible schedules', () => {
  it('keeps the activity feed compact while preserving exact report provenance', async () => {
    let now = 0;
    const api = createPreviewEvaluation(() => now);
    const input = plan();
    input.judge = { profileId: 'judge', modelId: 'review', reasoningEffort: null };
    const id = (await api.start(input)).active!.id;
    now += 3000;
    const records = (await api.activity()).records;
    expect(records).toHaveLength(3);
    expect(new Set(records.map((record) => record.id)).size).toBe(3);
    expect(records.every((record) => record.runId === id)).toBe(true);
    expect(records.find((record) => record.caseId === 'pelican')).toMatchObject({ hasArtifact: true, score: null, status: 'generated' });
    expect(records.every((record) => !('output' in record) && !('prompt' in record) && !('artifactHtml' in record))).toBe(true);
    const report = await api.run(id);
    expect(report.results.find((result) => result.caseId === 'pelican')!.judge).toBeNull();
    expect(report.results.find((result) => result.caseId === 'candy')!.judge).not.toBeNull();
  });
  it('defaults new schedules to thirty minutes and preserves old hour-based schedules', async () => {
    const api = createPreviewEvaluation(() => 0);
    expect((await api.dashboard()).plan.intervalMinutes).toBe(30);
    expect((await api.save({ ...plan(), intervalHours: 6, scheduleEnabled: true })).nextRunAt).toBe(new Date(6 * 3600000).toISOString());
    expect((await api.save({ ...plan(), intervalHours: 24, intervalMinutes: 30, scheduleEnabled: true })).nextRunAt).toBe(new Date(1800000).toISOString());
    for (const intervalMinutes of [10, 30, 10080]) expect(validateEvaluationPlan({ ...plan(), intervalMinutes })).toBeNull();
    for (const intervalMinutes of [9, 10081, 30.5]) expect(validateEvaluationPlan({ ...plan(), intervalMinutes })).toMatch('10 分钟–7 天');
  });
});
