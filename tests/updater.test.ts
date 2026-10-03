import { afterEach, describe, expect, it, vi } from 'vitest';
import { createPreviewUpdater, initialUpdateStatus, updatePercent, type UpdaterApi, type UpdateStatus } from '../src/lib/updater';
import { createUpdaterStore } from '../src/hooks/useUpdater';

afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); });

describe('memory-only update preview', () => {
  it('downloads in the background but never installs before an explicit action', async () => {
    let now = Date.parse('2026-10-03T10:00:00Z');
    const fetch = vi.fn(() => { throw new Error('Preview must not use the network'); });
    vi.stubGlobal('fetch', fetch);
    const api = createPreviewUpdater(() => now);
    expect((await api.check()).phase).toBe('checking');
    now += 500;
    expect((await api.status()).phase).toBe('downloading');
    now += 2500;
    expect((await api.status()).phase).toBe('ready');
    now += 60_000;
    expect((await api.status()).phase).toBe('ready');
    expect((await api.install()).phase).toBe('installing');
    now += 800;
    const finished = await api.status();
    expect(finished.phase).toBe('latest');
    expect(finished.currentVersion).toBe(finished.version);
    expect(fetch).not.toHaveBeenCalled();
  });

  it('respects manual download mode and saves preferences immediately', async () => {
    let now = 1;
    const api = createPreviewUpdater(() => now);
    await api.preferences(false);
    await api.check(); now += 500;
    expect((await api.status()).phase).toBe('available');
    now += 10_000;
    expect((await api.status()).downloadedBytes).toBe(0);
    await expect(api.install()).rejects.toThrow('请先完成演示下载');
    expect((await api.preferences(true)).phase).toBe('downloading');
    expect((await api.status()).autoDownload).toBe(true);
  });
});

describe('shared updater lifecycle', () => {
  function fixture() {
    vi.useFakeTimers();
    vi.stubGlobal('document', Object.assign(new EventTarget(), { hidden: false }));
    const initial = initialUpdateStatus();
    const api: UpdaterApi = {
      status: vi.fn(async () => initial), check: vi.fn(async (): Promise<UpdateStatus> => ({ ...initial, phase: 'checking' })),
      download: vi.fn(async (): Promise<UpdateStatus> => ({ ...initial, phase: 'downloading' })), install: vi.fn(async (): Promise<UpdateStatus> => ({ ...initial, phase: 'installing' })),
      preferences: vi.fn(async (autoDownload) => ({ ...initial, autoDownload })),
    };
    return { initial, api, store: createUpdaterStore(api) };
  }

  it('shares one poller and ignores a stale poll that finishes after a command', async () => {
    const { api, store, initial } = fixture();
    let resolveStatus!: (status: UpdateStatus) => void;
    vi.mocked(api.status).mockImplementationOnce(() => new Promise((resolve) => { resolveStatus = resolve; }));
    const unsubscribeFirst = store.subscribe(() => {});
    const unsubscribeSecond = store.subscribe(() => {});
    expect(api.status).toHaveBeenCalledTimes(1);
    await store.check();
    resolveStatus(initial);
    await Promise.resolve();
    expect(store.getSnapshot().status.phase).toBe('checking');
    unsubscribeFirst(); unsubscribeSecond();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(api.status).toHaveBeenCalledTimes(1);
  });

  it('retains a prepared update after an install error and allows retry', async () => {
    const { api, store, initial } = fixture();
    vi.mocked(api.download).mockResolvedValue({ ...initial, phase: 'ready', version: '0.4.1' });
    await store.download();
    vi.mocked(api.install).mockRejectedValueOnce(new Error('安装器启动失败'));
    await store.install();
    expect(store.getSnapshot().status.phase).toBe('ready');
    expect(store.getSnapshot().requestError).toBe('安装器启动失败');
    await store.install();
    expect(store.getSnapshot().status.phase).toBe('installing');
    expect(store.getSnapshot().requestError).toBeNull();
  });

  it('prevents overlapping user commands', async () => {
    const { api, store, initial } = fixture();
    let resolveCheck!: (status: UpdateStatus) => void;
    vi.mocked(api.check).mockImplementationOnce(() => new Promise((resolve) => { resolveCheck = resolve; }));
    const pending = store.check();
    await store.check();
    await store.download();
    expect(api.check).toHaveBeenCalledTimes(1);
    expect(api.download).not.toHaveBeenCalled();
    resolveCheck({ ...initial, phase: 'checking' });
    await pending;
  });
});

describe('download progress', () => {
  it('shows indeterminate progress without a reliable content length', () => {
    expect(updatePercent({ downloadedBytes: 120, totalBytes: null })).toBeUndefined();
    expect(updatePercent({ downloadedBytes: 120, totalBytes: 0 })).toBeUndefined();
  });
  it('clamps progress to the valid range', () => {
    expect(updatePercent({ downloadedBytes: 150, totalBytes: 100 })).toBe(100);
    expect(updatePercent({ downloadedBytes: -10, totalBytes: 100 })).toBe(0);
    expect(updatePercent({ downloadedBytes: 33, totalBytes: 100 })).toBe(33);
  });
});
