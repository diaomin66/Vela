import { useEffect, useMemo, useRef } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { evaluationApi, type EvaluationDashboard, type EvaluationPlan, type EvaluationRecord } from '../lib/evaluation';
import { recordsForRun } from '../lib/evaluation/records';
import { errorMessage } from '../lib/utils';

export const evaluationKeys = {
  dashboard: ['evaluations', 'dashboard'] as const,
  activity: ['evaluations', 'activity'] as const,
  run: (id: string | null) => ['evaluations', 'run', id] as const,
};

type Operation = { kind: 'save' | 'start'; plan: EvaluationPlan } | { kind: 'cancel'; runId: string } | { kind: 'remove'; runIds: string[] };

export function useEvaluations() {
  const client = useQueryClient();
  const locked = useRef(false);
  const previousActive = useRef<string | null>(null);
  const dashboard = useQuery({
    queryKey: evaluationKeys.dashboard,
    queryFn: () => evaluationApi.dashboard(),
    refetchInterval: (query) => query.state.data?.active ? 700 : 15_000,
  });
  const historyVersion = dashboard.data?.history.map((run) => `${run.id}:${run.status}`).join('|') ?? '';
  const feed = useQuery({
    queryKey: evaluationKeys.activity,
    queryFn: () => evaluationApi.activity(),
    enabled: Boolean(dashboard.data),
    refetchInterval: 15_000,
  });
  useEffect(() => {
    if (dashboard.data) void client.invalidateQueries({ queryKey: evaluationKeys.activity });
  }, [client, historyVersion, Boolean(dashboard.data)]);
  useEffect(() => {
    const active = dashboard.data?.active;
    if (active) client.setQueryData(evaluationKeys.run(active.id), active);
    if (previousActive.current && previousActive.current !== active?.id) {
      void client.invalidateQueries({ queryKey: evaluationKeys.run(previousActive.current) });
    }
    previousActive.current = active?.id ?? null;
  }, [client, dashboard.data?.active]);
  const mutation = useMutation({
    mutationFn: (operation: Operation) => operation.kind === 'remove'
      ? evaluationApi.remove(operation.runIds)
      : operation.kind === 'cancel' ? evaluationApi.cancel(operation.runId)
      : evaluationApi[operation.kind](operation.plan),
    onMutate: async (operation: Operation) => {
      await Promise.all([
        client.cancelQueries({ queryKey: evaluationKeys.dashboard }),
        client.cancelQueries({ queryKey: evaluationKeys.activity }),
        ...(operation.kind === 'remove' ? operation.runIds.map((id) => client.cancelQueries({ queryKey: evaluationKeys.run(id), exact: true })) : []),
      ]);
    },
    onSuccess: (data: EvaluationDashboard, operation: Operation) => {
      client.setQueryData(evaluationKeys.dashboard, data);
      if (operation.kind === 'remove') {
        const ids = new Set(operation.runIds);
        for (const id of ids) client.removeQueries({ queryKey: evaluationKeys.run(id), exact: true });
        client.setQueryData(evaluationKeys.activity, (previous: { records: EvaluationRecord[] } | undefined) => previous ? { records: previous.records.filter((record) => !ids.has(record.runId)) } : previous);
      }
      void client.invalidateQueries({ queryKey: evaluationKeys.activity });
    },
    onSettled: () => {
      void client.invalidateQueries({ queryKey: evaluationKeys.dashboard });
    },
  });
  async function act(operation: Operation) {
    if (locked.current) return false;
    locked.current = true;
    try { await mutation.mutateAsync(operation); return true; }
    catch { return false; }
    finally { locked.current = false; }
  }
  const activity = useMemo(() => {
    if (!feed.data) return null;
    const active = dashboard.data?.active;
    if (!active) return feed.data;
    return { records: [...recordsForRun(active), ...feed.data.records.filter((record) => record.runId !== active.id)] };
  }, [feed.data, dashboard.data?.active]);
  const error = mutation.error ?? dashboard.error ?? feed.error;
  return {
    data: dashboard.data ?? null,
    activity,
    pending: mutation.isPending,
    refreshing: dashboard.isFetching || feed.isFetching,
    error: error ? errorMessage(error) : null,
    refresh: async () => { mutation.reset(); await Promise.all([dashboard.refetch(), feed.refetch()]); },
    save: (plan: EvaluationPlan) => act({ kind: 'save', plan }),
    start: (plan: EvaluationPlan) => act({ kind: 'start', plan }),
    cancel: (runId: string) => act({ kind: 'cancel', runId }),
    remove: (runIds: string[]) => act({ kind: 'remove', runIds }),
  };
}

export function useEvaluationRun(runId: string | null) {
  return useQuery({
    queryKey: evaluationKeys.run(runId),
    queryFn: () => evaluationApi.run(runId!),
    enabled: Boolean(runId),
    staleTime: 60_000,
    refetchInterval: (query) => query.state.data?.status === 'running' ? 1_000 : false,
  });
}
