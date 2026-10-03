import { useSyncExternalStore } from 'react';
import { evaluationApi, type EvaluationApi, type EvaluationDashboard, type EvaluationPlan } from '../lib/evaluation';
import { errorMessage } from '../lib/utils';

interface Snapshot { data: EvaluationDashboard | null; pending: boolean; error: string | null }
export function createEvaluationStore(api: EvaluationApi) {
  let snapshot: Snapshot = { data: null, pending: false, error: null };
  let generation = 0;
  let reading = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const listeners = new Set<() => void>();
  const publish = (value: Snapshot) => { snapshot = value; listeners.forEach((listener) => listener()); };
  function schedule() {
    clearTimeout(timer);
    if (listeners.size) timer = setTimeout(() => void refresh(), snapshot.data?.active ? 700 : 15000);
  }
  async function refresh() {
    if (reading || snapshot.pending || document.hidden) { schedule(); return; }
    const request = generation; reading = true;
    try { const data = await api.dashboard(); if (generation === request) publish({ ...snapshot, data, error: null }); }
    catch (error) { if (generation === request) publish({ ...snapshot, error: errorMessage(error) }); }
    finally { reading = false; schedule(); }
  }
  function visible() { if (!document.hidden) void refresh(); }
  async function act(operation: () => Promise<EvaluationDashboard>) {
    if (snapshot.pending) return false;
    generation++; publish({ ...snapshot, pending: true, error: null });
    try { publish({ data: await operation(), pending: false, error: null }); return true; }
    catch (error) { publish({ ...snapshot, pending: false, error: errorMessage(error) }); return false; }
    finally { schedule(); }
  }
  return {
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) { listeners.add(listener); if (listeners.size === 1) { document.addEventListener('visibilitychange', visible); void refresh(); } return () => { listeners.delete(listener); if (!listeners.size) { clearTimeout(timer); document.removeEventListener('visibilitychange', visible); } }; },
    refresh,
    save: (plan: EvaluationPlan) => act(() => api.save(plan)),
    start: (plan: EvaluationPlan) => act(() => api.start(plan)),
    cancel: (runId: string) => act(() => api.cancel(runId)),
  };
}
const store = createEvaluationStore(evaluationApi);
export function useEvaluations() { return { ...useSyncExternalStore(store.subscribe, store.getSnapshot), refresh: store.refresh, save: store.save, start: store.start, cancel: store.cancel }; }
