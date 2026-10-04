import type { ThreadDashboard, ThreadDeletionPreview, ThreadDeletionResult, ThreadRestorePreview, ThreadSettings, ThreadSummary, ThreadTrashItem, ThreadTrashPage, ThreadTrashRestorePreview, ThreadsApi } from './types';
import { threadNeedsAttention, threadTitle } from './presentation';

export function createPreviewThreads(now: () => number = Date.now, empty = false, scenario: 'standard' | 'batch' | 'delete' | 'interrupted' = 'standard'): ThreadsApi {
  let settings: ThreadSettings = { enabled: true, intervalSeconds: 60, protectBeforeConfigurationChange: true, includeArchived: true };
  let revision = 1;
  let deletionRevision = 1;
  let scannedAt = empty ? null : new Date(now()).toISOString();
  let simulatedWriteFailure = scenario === 'batch';
  const titles = ['工作台导航与交互整理', '订单服务的性能分析', '项目文档与发布说明', '恢复上周的设计讨论', '团队知识库的检索体验', 'Windows 安装流程检查', '接口错误处理与重试', '仪表盘的浅色与深色主题'];
  const threads: ThreadSummary[] = empty ? [] : Array.from({ length: 28 }, (_, index) => {
    const sourceId = index % 5 === 4 ? 'demo-previous' : 'demo-current';
    const root = sourceId === 'demo-current' ? 'C:\\Users\\Demo\\.codex' : 'D:\\Archive\\.codex';
    const threadId = `019a0000-0000-7000-8000-${String(index + 1).padStart(12, '0')}`;
    const relativePath = `${index > 22 ? 'archived_sessions' : 'sessions/2026/10/03'}/rollout-2026-10-03T12-00-00-${threadId}.jsonl`;
    const missing = index === 3 || scenario === 'batch' && [7, 11].includes(index);
    return {
      key: `${sourceId}:${threadId}`, sourceId, threadId, path: `${root}\\${relativePath.replaceAll('/', '\\')}`, relativePath,
      archived: index > 22, title: `${titles[index % titles.length]}${index > 7 ? ` · ${Math.floor(index / 8) + 1}` : ''}`,
      cwd: index % 3 ? 'D:\\Projects\\Workspace' : 'D:\\Projects\\Atlas', provider: index % 4 ? 'Vela' : 'openai', sourceKind: 'vscode',
      createdAt: new Date(now() - (index + 4) * 86400000).toISOString(), updatedAt: new Date(now() - (index + 1) * 7200000).toISOString(),
      bytes: 24000 + index * 4700, lineCount: 90 + index * 8, indexPresent: index !== 3 && index !== 6,
      integrity: missing ? 'missing' : index === 6 ? 'partial' : 'valid', snapshot: index === 6 ? 'pending' : 'protected',
      recoverability: missing ? 'recoverable' : index === 6 ? 'unavailable' : 'source-present', fingerprint: `demo-hash-${index}`, scanRevision: String(revision),
    };
  });
  if (scenario === 'delete') {
    const original = threads[0];
    threads.push({ ...original, key: original.key + ':history', selectedRollout: false, relativePath: 'sessions/history-' + original.threadId + '.jsonl', path: original.path.replace('rollout-', 'history-'), title: original.title + ' · 历史版本' });
  }
  const trash: ThreadTrashItem[] = [];
  const removed = new Map<string, ThreadSummary[]>();
  let simulatedDeletionFailure = scenario === 'delete';
  let simulatedRestoreFailure = scenario === 'delete';
  const deletionKey = (keys: string[]) => `${deletionRevision}:${[...new Set(keys)].sort().join('|')}`;
  function find(key: string) {
    const value = threads.find((thread) => thread.key === key);
    if (!value) throw new Error('线程记录不存在，请重新扫描。');
    return value;
  }
  function dashboard(): ThreadDashboard {
    const protectedThreads = threads.filter((thread) => thread.snapshot === 'protected');
    return structuredClone({
      sources: [{ id: 'demo-current', kind: 'current', root: 'C:\\Users\\Demo\\.codex', displayRoot: '当前数据目录', available: true, writable: true, lastScannedAt: scannedAt, error: null }, { id: 'demo-previous', kind: 'discovered', root: 'D:\\Archive\\.codex', displayRoot: '历史数据目录', available: true, writable: false, lastScannedAt: scannedAt, error: null }],
      threads: [], scannedAt, scanRevision: String(revision), total: threads.length, protected: protectedThreads.length,
      recoverable: threads.filter((thread) => thread.recoverability === 'recoverable').length,
      attention: threads.filter((thread) => thread.integrity !== 'valid' || thread.snapshot !== 'protected').length,
      protection: { state: settings.enabled ? 'idle' : 'paused', lastSuccessAt: scannedAt, lastAttemptAt: scannedAt, protectedCount: protectedThreads.length, pendingCount: threads.length - protectedThreads.length, failedCount: 0, bytesProtected: protectedThreads.reduce((sum, thread) => sum + thread.bytes, 0), currentPath: null, error: null }, error: null,
    });
  }
  function preview(key: string): ThreadRestorePreview {
    const thread = find(key);
    if (thread.snapshot !== 'protected') throw new Error('这条线程没有可用的完整快照。');
    const targetExists = thread.integrity !== 'missing';
    const conflict = scenario === 'batch' && thread.threadId.endsWith('000000000012');
    return structuredClone({ thread, snapshotHash: thread.fingerprint, targetPath: thread.path, targetExists: targetExists || conflict, targetHash: conflict ? 'different-content' : targetExists ? thread.fingerprint : null, conflict, expectedHash: `${thread.key}:${thread.fingerprint}:${targetExists ? thread.fingerprint : 'missing'}`, warning: conflict ? '目标位置已有不同内容，本次不会覆盖。' : targetExists ? '现有文件与备份一致，不需要覆盖。' : null });
  }
  function previewDeletion(keys: string[]): ThreadDeletionPreview {
    const selected = [...new Set(keys)].map(find);
    const groups = new Map<string, ThreadSummary[]>();
    for (const thread of selected) {
      const group = `${thread.sourceId}:${thread.threadId}`;
      groups.set(group, threads.filter((candidate) => candidate.sourceId === thread.sourceId && candidate.threadId === thread.threadId));
    }
    const items = [...groups.values()].map((members) => {
      const blocked = scenario === 'delete' && members.some((thread) => thread.threadId.endsWith('000000000005'));
      return { key: members[0].key, threadId: members[0].threadId, sourceId: members[0].sourceId, title: threadTitle(members[0]), rolloutCount: members.length, bytes: members.reduce((sum, thread) => sum + thread.bytes, 0), canDelete: !blocked, reason: blocked ? '这条线程是其他历史记录的基础，需先处理依赖它的后代线程。' : undefined };
    });
    return structuredClone({ expectedHash: deletionKey(keys), items, logicalCount: items.length, rolloutCount: items.reduce((sum, item) => sum + item.rolloutCount, 0), warning: '整条线程的所有记录文件会移入 AhaX 回收站。保护副本保留，可检查后撤销删除。' });
  }
  return {
    async dashboard() { return dashboard(); },
    async list(query) {
      if (!Number.isInteger(query.limit) || query.limit < 1 || query.limit > 100 || !Number.isInteger(query.offset) || query.offset < 0) throw new Error('分页参数无效。');
      const search = query.search.trim().toLocaleLowerCase();
      const matching = threads.filter((thread) => (!query.sourceId || thread.sourceId === query.sourceId) && (query.scope === 'all' || thread.archived === (query.scope === 'archived')) && (query.status === 'all' || query.status === 'attention' && threadNeedsAttention(thread) || query.status === 'protected' && thread.snapshot === 'protected' || query.status === 'recoverable' && thread.recoverability === 'recoverable') && (!search || `${threadTitle(thread)} ${thread.cwd ?? ''} ${thread.provider ?? ''} ${thread.threadId}`.toLocaleLowerCase().includes(search))).sort((left, right) => (Date.parse(right.updatedAt ?? '') || 0) - (Date.parse(left.updatedAt ?? '') || 0) || left.key.localeCompare(right.key));
      return structuredClone({ threads: matching.slice(query.offset, query.offset + query.limit), total: matching.length, offset: query.offset, limit: query.limit, scanRevision: String(revision) });
    },
    async rebuild() { revision += 1; return dashboard(); },
    async open(key) {
      const thread = find(key);
      if (thread.sourceId !== 'demo-current') throw new Error('这条线程属于另一个数据目录，请先在 Codex 中切换到对应目录。');
      if (thread.integrity !== 'valid') throw new Error('线程原文件尚不可用，请先恢复或完成检查。');
      return '演示模式不会打开 Codex。桌面版会进入这条原生对话。';
    },
    async previewDeletion(keys) { return previewDeletion(keys); },
    async deleteThreads(keys, expectedHash) {
      if (deletionKey(keys) !== expectedHash) throw new Error('线程列表已经变化，请重新预览删除。');
      const preview = previewDeletion(keys);
      const results: ThreadDeletionResult['items'] = [];
      for (const item of preview.items) {
        if (!item.canDelete) { results.push({ key: item.key, threadId: item.threadId, status: 'blocked', message: item.reason || '当前线程存在依赖，未删除。' }); continue; }
        if (simulatedDeletionFailure && item.threadId.endsWith('000000000002')) { simulatedDeletionFailure = false; results.push({ key: item.key, threadId: item.threadId, status: 'failed', message: '演示：记录文件暂时被占用，请重新检查。' }); continue; }
        const group = threads.filter((thread) => thread.sourceId === item.sourceId && thread.threadId === item.threadId);
        const trashId = `trash-${crypto.randomUUID()}`;
        const interrupted = scenario === 'interrupted' && item.threadId.endsWith('000000000001');
        removed.set(trashId, group.map((thread) => structuredClone(thread)));
        for (const thread of group) { const index = threads.findIndex((current) => current.key === thread.key); if (index >= 0) threads.splice(index, 1); }
        const message = interrupted ? '删除未完全确认：本机服务响应中断。加密副本已保留，可在回收站预览撤销。' : '已移入 AhaX 回收站。';
        trash.unshift({ id: trashId, threadId: item.threadId, sourceId: item.sourceId, title: item.title || null, deletedAt: new Date(now()).toISOString(), rolloutCount: item.rolloutCount, bytes: item.bytes, state: interrupted ? 'interrupted' : 'deleted', message: interrupted ? message : undefined });
        results.push({ key: item.key, threadId: item.threadId, status: interrupted ? 'interrupted' : 'deleted', trashId, message });
      }
      deletionRevision += 1; revision += 1;
      return { items: results, deletedCount: results.filter((item) => item.status === 'deleted').length, failedCount: results.filter((item) => item.status !== 'deleted').length };
    },
    async trash(query = {}) {
      const offset = query.offset ?? 0; const limit = Math.min(100, Math.max(1, query.limit ?? 20));
      return structuredClone({ items: trash.slice(offset, offset + limit), total: trash.length, offset, limit } satisfies ThreadTrashPage);
    },
    async previewTrashRestore(id) {
      const item = trash.find((entry) => entry.id === id);
      if (!item) throw new Error('回收站记录不存在，请重新读取。');
      return { id, threadId: item.threadId, rolloutCount: item.rolloutCount, expectedHash: `${deletionRevision}:${id}`, canRestore: ['deleted', 'prepared', 'interrupted', 'restoring'].includes(item.state), warning: '撤销会恢复会话原始记录并重新加入保护清单；独立附件与目标元数据不保证完整还原。官方 Codex 列表可能仍需重新索引。' } satisfies ThreadTrashRestorePreview;
    },
    async restoreTrash(id, expectedHash) {
      if (`${deletionRevision}:${id}` !== expectedHash) throw new Error('回收站状态已经变化，请重新预览。');
      const item = trash.find((entry) => entry.id === id); if (!item) throw new Error('回收站记录不存在。');
      const recovered = removed.get(id) || [];
      if (simulatedRestoreFailure) {
        simulatedRestoreFailure = false;
        item.state = 'interrupted';
        const partial: ThreadDeletionResult['items'] = recovered.length > 1 ? [{ key: recovered[0].key, threadId: item.threadId, status: 'restored', message: '一份记录已恢复，仍需完成其余记录。' }] : [];
        partial.push({ key: recovered[1]?.key || recovered[0]?.key || id, threadId: item.threadId, status: 'failed', message: '演示：目标位置暂时不可写，尚有记录未恢复。请重新检查。' });
        return { items: partial, deletedCount: partial.length - 1, failedCount: 1 };
      }
      threads.push(...recovered.map((thread) => structuredClone(thread)));
      item.state = 'restoring';
      const message = scenario === 'interrupted' ? '会话记录已恢复并通过校验。官方列表尚未刷新：本机线程服务暂不可用，请在数据来源中重新索引。' : '已撤销删除并重新加入保护清单。';
      const result = { items: recovered.map((thread) => ({ key: thread.key, threadId: item.threadId, status: 'restored' as const, message })), deletedCount: recovered.length, failedCount: 0 };
      trash.splice(trash.findIndex((entry) => entry.id === id), 1); removed.delete(id); deletionRevision += 1; revision += 1;
      return result;
    },
    async scan() {
      revision += 1; scannedAt = new Date(now()).toISOString();
      for (const thread of threads) thread.scanRevision = String(revision);
      return dashboard();
    },
    async detail(key) { const thread = find(key); return structuredClone({ summary: thread, preview: [], snapshotBytes: thread.snapshot === 'protected' ? thread.bytes : null, snapshotHash: thread.snapshot === 'protected' ? thread.fingerprint : null, rawAvailable: thread.integrity !== 'missing' }); },
    async previewRestore(key) { return preview(key); },
    async restore(key, expectedHash) {
      const current = preview(key);
      if (current.expectedHash !== expectedHash) throw new Error('线程状态已变化，请重新预览后恢复。');
      if (current.conflict) throw new Error('目标文件存在不同内容，本次未覆盖。');
      if (simulatedWriteFailure && current.thread.threadId.endsWith('000000000008')) { simulatedWriteFailure = false; throw new Error('演示：目标目录暂时不可写。请重新预览后重试。'); }
      const thread = find(key);
      thread.integrity = 'valid'; thread.recoverability = 'source-present'; thread.indexPresent = true;
      revision += 1; thread.scanRevision = String(revision);
      return dashboard();
    },
    async settings() { return structuredClone(settings); },
    async saveSettings(input) {
      if (!Number.isInteger(input.intervalSeconds) || input.intervalSeconds < 15 || input.intervalSeconds > 3600) throw new Error('线程保护间隔需为 15–3600 秒。');
      settings = structuredClone(input);
      return dashboard();
    },
    async reconcile(sourceId) {
      if (sourceId !== 'demo-current') throw new Error('这个数据目录当前只读，无法重新索引。');
      const sourceThreads = threads.filter((thread) => thread.sourceId === sourceId && thread.integrity !== 'missing');
      return { sourceId, activeCount: sourceThreads.filter((thread) => !thread.archived).length, archivedCount: sourceThreads.filter((thread) => thread.archived).length, completedAt: new Date(now()).toISOString(), message: '演示索引检查完成。桌面版会调用本机服务核对实际索引。' };
    },
  };
}
