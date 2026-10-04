import type { ThreadDashboard, ThreadRestorePreview, ThreadSettings, ThreadSummary, ThreadsApi } from './types';
import { threadNeedsAttention, threadTitle } from './presentation';

export function createPreviewThreads(now: () => number = Date.now, empty = false, scenario: 'standard' | 'batch' = 'standard'): ThreadsApi {
  let settings: ThreadSettings = { enabled: true, intervalSeconds: 60, protectBeforeConfigurationChange: true, includeArchived: true };
  let revision = 1;
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
