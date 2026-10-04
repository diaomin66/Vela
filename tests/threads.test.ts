import { afterEach, describe, expect, it, vi } from 'vitest';
import { createPreviewThreads } from '../src/lib/threads/preview';
import { canRestoreThread, integrityLabel, threadDependencyIssue, threadNeedsAttention, threadStatus } from '../src/lib/threads/presentation';
import type { ThreadListQuery } from '../src/lib/threads/types';

const query: ThreadListQuery = { search: '', scope: 'all', status: 'all', sourceId: null, offset: 0, limit: 20 };
afterEach(() => vi.unstubAllGlobals());

describe('thread inventory', () => {
  it('serves bounded pages without embedding the inventory in every status poll', async () => {
    const api = createPreviewThreads(() => 1791028800000);
    const dashboard = await api.dashboard();
    expect(dashboard.threads).toEqual([]);
    expect(dashboard.total).toBe(28);
    const first = await api.list(query);
    const second = await api.list({ ...query, offset: 20 });
    expect(first.threads).toHaveLength(20);
    expect(second.threads).toHaveLength(8);
    expect(new Set([...first.threads, ...second.threads].map((thread) => thread.key)).size).toBe(28);
    expect(first.total).toBe(28);
    await expect(api.list({ ...query, limit: 101 })).rejects.toThrow('分页');
    await expect(api.list({ ...query, offset: -1 })).rejects.toThrow('分页');
  });

  it('combines source, archive and text filters before pagination', async () => {
    const api = createPreviewThreads(() => 1791028800000);
    const archived = await api.list({ ...query, scope: 'archived' });
    expect(archived.total).toBe(5);
    expect(archived.threads.every((thread) => thread.archived)).toBe(true);
    const previous = await api.list({ ...query, sourceId: 'demo-previous' });
    expect(previous.threads.every((thread) => thread.sourceId === 'demo-previous')).toBe(true);
    const byId = await api.list({ ...query, search: previous.threads[0].threadId });
    expect(byId.threads.map((thread) => thread.key)).toEqual([previous.threads[0].key]);
    const both = await api.list({ ...query, search: '知识库', sourceId: 'demo-current' });
    expect(both.threads.every((thread) => thread.sourceId === 'demo-current' && thread.title?.includes('知识库'))).toBe(true);
  });

  it('only offers recovery for missing records with a verified snapshot', async () => {
    const api = createPreviewThreads(() => 1791028800000);
    const { threads } = await api.list(query);
    const present = threads.find((thread) => thread.integrity === 'valid')!;
    expect(canRestoreThread(present)).toBe(false);
    expect(threadNeedsAttention({ ...present, indexPresent: false })).toBe(false);
    expect(threadStatus({ ...present, indexPresent: false }).label).toBe('已有备份');
    expect(threadNeedsAttention({ ...present, stateIndex: 'missing' })).toBe(true);
    expect(threadStatus({ ...present, stateIndex: 'missing' }).label).toBe('列表索引缺失');
    const missing = threads.find((thread) => thread.recoverability === 'recoverable')!;
    expect(canRestoreThread(missing)).toBe(true);
    const preview = await api.previewRestore(missing.key);
    expect(preview.targetExists).toBe(false);
    await api.scan();
    await expect(api.restore(missing.key, `${preview.expectedHash}:stale`)).rejects.toThrow('状态已变化');
    expect((await api.detail(missing.key)).rawAvailable).toBe(false);
    await api.restore(missing.key, preview.expectedHash);
    await expect(api.restore(missing.key, preview.expectedHash)).rejects.toThrow('状态已变化');
    const recovered = await api.detail(missing.key);
    expect(recovered.rawAvailable).toBe(true);
    expect(recovered.summary.threadId).toBe(missing.threadId);
    expect(canRestoreThread(recovered.summary)).toBe(false);
  });

  it('keeps missing or circular history dependencies out of recovery even when the current file has a snapshot', async () => {
    const api = createPreviewThreads(() => 1791028800000);
    const present = (await api.list(query)).threads[0];
    const historyBase = { threadId: '019a0000-0000-7000-8000-000000000099', endOrdinalExclusive: 42 };
    for (const integrity of ['dependency-missing', 'dependency-cycle']) {
      const dependent = { ...present, integrity, historyBase, recoverability: 'recoverable' };
      expect(canRestoreThread(dependent)).toBe(false);
      expect(threadNeedsAttention(dependent)).toBe(true);
      expect(threadStatus(dependent).label).toBe(integrity === 'dependency-cycle' ? '历史引用循环' : '历史基础缺失');
      expect(integrityLabel(integrity)).toContain('历史');
    }
    const unavailable = { ...present, historyBase, recoverability: 'dependencies-missing' };
    expect(threadDependencyIssue(unavailable)).toBe('missing');
    expect(canRestoreThread(unavailable)).toBe(false);
    expect(threadNeedsAttention(unavailable)).toBe(true);
    expect(threadStatus(unavailable).label).toBe('历史基础缺失');
  });

  it('keeps demo settings independent and never calls a model or the filesystem', async () => {
    const fetch = vi.fn(() => { throw new Error('Unexpected network'); });
    vi.stubGlobal('fetch', fetch);
    const api = createPreviewThreads(() => 1791028800000);
    const settings = { ...await api.settings(), intervalSeconds: 125, enabled: false, includeArchived: false };
    await api.saveSettings(settings);
    settings.intervalSeconds = 300;
    expect((await api.settings()).intervalSeconds).toBe(125);
    expect((await api.dashboard()).protection.state).toBe('paused');
    await expect(api.saveSettings({ ...settings, intervalSeconds: 14 })).rejects.toThrow('15–3600');
    await expect(api.saveSettings({ ...settings, intervalSeconds: 30.5 })).rejects.toThrow('15–3600');
    const result = await api.reconcile('demo-current');
    expect(result.activeCount).toBeGreaterThan(0);
    await expect(api.reconcile('demo-previous')).rejects.toThrow('只读');
    const active = (await api.list(query)).threads;
    await expect(api.open(active[0].key)).resolves.toContain('演示模式');
    await expect(api.open(active.find((thread) => thread.sourceId === 'demo-previous')!.key)).rejects.toThrow('另一个数据目录');
    await expect(api.open(active.find((thread) => thread.recoverability === 'recoverable')!.key)).rejects.toThrow('尚不可用');
    expect(fetch).not.toHaveBeenCalled();
    const other = createPreviewThreads(() => 1791028800000, true);
    expect((await other.settings()).enabled).toBe(true);
    expect((await other.list(query)).total).toBe(0);
  });
});
