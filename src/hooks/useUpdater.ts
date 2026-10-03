import { useSyncExternalStore } from 'react';
import { errorMessage } from '../lib/utils';
import { initialUpdateStatus, updaterApi, type UpdateStatus, type UpdaterApi } from '../lib/updater';

interface Snapshot { status: UpdateStatus; pending: boolean; requestError: string | null }

// All consumers share one poller. The native service owns automatic checks and
// downloads, including while the window is hidden or no view is mounted.
export function createUpdaterStore(api: UpdaterApi) {
  let snapshot: Snapshot = { status: initialUpdateStatus(), pending: false, requestError: null };
  const listeners = new Set<() => void>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let reading = false;
  let generation = 0;
  function publish(next: Snapshot) {
    if (JSON.stringify(snapshot) === JSON.stringify(next)) return;
    snapshot = next;
    listeners.forEach((listener) => listener());
  }
  function schedule() {
    clearTimeout(timer);
    if (!listeners.size) return;
    const active = ['checking', 'downloading', 'installing'].includes(snapshot.status.phase);
    timer = setTimeout(() => void refresh(), active ? 700 : 15_000);
  }
  async function refresh() {
    if (reading || snapshot.pending || document.hidden) { schedule(); return; }
    const request = generation;
    reading = true;
    try {
      const status = await api.status();
      if (request === generation) publish({ ...snapshot, status, requestError: null });
    } catch (error) {
      if (request === generation) publish({ ...snapshot, requestError: errorMessage(error) });
    } finally { reading = false; schedule(); }
  }
  function visible() { if (!document.hidden) void refresh(); }
  async function act(operation: () => Promise<UpdateStatus>) {
    if (snapshot.pending) return;
    ++generation;
    publish({ ...snapshot, pending: true, requestError: null });
    try { publish({ status: await operation(), pending: false, requestError: null }); }
    catch (error) { publish({ ...snapshot, pending: false, requestError: errorMessage(error) }); }
    finally { schedule(); }
  }
  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      if (listeners.size === 1) { document.addEventListener('visibilitychange', visible); void refresh(); }
      return () => {
        listeners.delete(listener);
        if (!listeners.size) { clearTimeout(timer); document.removeEventListener('visibilitychange', visible); }
      };
    },
    getSnapshot: () => snapshot,
    check: () => act(() => api.check()),
    download: () => act(() => api.download()),
    install: () => act(() => api.install()),
    preferences: (autoDownload: boolean) => act(() => api.preferences(autoDownload)),
  };
}

const updaterStore = createUpdaterStore(updaterApi);

export function useUpdater() {
  const snapshot = useSyncExternalStore(updaterStore.subscribe, updaterStore.getSnapshot);
  return { ...snapshot, check: updaterStore.check, download: updaterStore.download, install: updaterStore.install, preferences: updaterStore.preferences };
}
