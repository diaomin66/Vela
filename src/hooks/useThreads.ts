import { useEffect, useRef } from 'react';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { threadsApi } from '../lib/threads/api';
import type { ThreadListQuery, ThreadSettings } from '../lib/threads/types';
import { errorMessage } from '../lib/utils';

export const threadKeys = {
  all: ['threads'] as const,
  dashboard: ['threads', 'dashboard'] as const,
  settings: ['threads', 'settings'] as const,
  detail: (key: string) => ['threads', 'detail', key] as const,
  restore: (key: string) => ['threads', 'restore', key] as const,
  list: (query: ThreadListQuery) => ['threads', 'list', query] as const,
};

type Operation = { kind: 'scan' } | { kind: 'rebuild' } | { kind: 'restore'; key: string; expectedHash: string } | { kind: 'settings'; settings: ThreadSettings };

export function useThreads(query: ThreadListQuery) {
  const client = useQueryClient();
  const locked = useRef(false);
  const dashboard = useQuery({ queryKey: threadKeys.dashboard, queryFn: threadsApi.dashboard, refetchInterval: (query) => query.state.data?.protection.state === 'scanning' ? 1000 : 15_000 });
  const settings = useQuery({ queryKey: threadKeys.settings, queryFn: threadsApi.settings });
  const list = useQuery({ queryKey: threadKeys.list(query), queryFn: () => threadsApi.list(query), enabled: Boolean(dashboard.data), placeholderData: keepPreviousData });
  useEffect(() => { void client.invalidateQueries({ queryKey: ['threads', 'list'] }); }, [client, dashboard.data?.scanRevision]);
  const mutation = useMutation({
    mutationFn: (operation: Operation) => operation.kind === 'scan' ? threadsApi.scan() : operation.kind === 'rebuild' ? threadsApi.rebuild() : operation.kind === 'restore' ? threadsApi.restore(operation.key, operation.expectedHash) : threadsApi.saveSettings(operation.settings),
    onMutate: () => client.cancelQueries({ queryKey: threadKeys.all }),
    onSuccess: (data, operation) => {
      client.setQueryData(threadKeys.dashboard, data);
      if (operation.kind === 'settings') client.setQueryData(threadKeys.settings, operation.settings);
      if (operation.kind === 'rebuild') void client.invalidateQueries({ queryKey: threadKeys.settings });
      void client.invalidateQueries({ queryKey: ['threads', 'detail'] });
      void client.invalidateQueries({ queryKey: ['threads', 'restore'] });
      void client.invalidateQueries({ queryKey: ['threads', 'list'] });
    },
    onSettled: () => { void client.invalidateQueries({ queryKey: threadKeys.dashboard }); },
  });
  async function act(operation: Operation) {
    if (locked.current) return false;
    locked.current = true;
    try { await mutation.mutateAsync(operation); return true; }
    catch { return false; }
    finally { locked.current = false; }
  }
  const error = mutation.error ?? dashboard.error ?? settings.error ?? list.error;
  return {
    data: dashboard.data ?? null,
    settings: settings.data ?? null,
    list: list.data ?? null,
    loadingList: list.isPending,
    refreshingList: list.isFetching,
    dashboardError: dashboard.error ? errorMessage(dashboard.error) : null,
    pending: mutation.isPending,
    scanning: mutation.isPending && mutation.variables?.kind === 'scan' || dashboard.data?.protection.state === 'scanning',
    error: error ? errorMessage(error) : null,
    resetError: mutation.reset,
    refresh: async () => { mutation.reset(); await Promise.all([dashboard.refetch(), settings.refetch(), list.refetch()]); },
    scan: () => act({ kind: 'scan' }),
    restore: (key: string, expectedHash: string) => act({ kind: 'restore', key, expectedHash }),
    saveSettings: (value: ThreadSettings) => act({ kind: 'settings', settings: value }),
    rebuild: () => act({ kind: 'rebuild' }),
  };
}

export function useThreadDetail(key: string) { return useQuery({ queryKey: threadKeys.detail(key), queryFn: () => threadsApi.detail(key), staleTime: 0 }); }
export function useThreadRestore(key: string) { return useQuery({ queryKey: threadKeys.restore(key), queryFn: () => threadsApi.previewRestore(key), staleTime: 0 }); }
