import { errorMessage } from '../utils';
import { canRestoreThread } from './presentation';
import type { ThreadRestorePreview, ThreadSummary, ThreadsApi } from './types';

export interface ThreadBatchEntry {
  thread: ThreadSummary;
  state: 'ready' | 'blocked' | 'restoring' | 'success' | 'failed';
  preview: ThreadRestorePreview | null;
  message: string | null;
}

export async function previewThreadBatch(api: ThreadsApi, threads: ThreadSummary[]): Promise<ThreadBatchEntry[]> {
  const entries: ThreadBatchEntry[] = [];
  for (const thread of threads) {
    try {
      const preview = await api.previewRestore(thread.key);
      const blocked = preview.conflict || preview.targetExists || !canRestoreThread(preview.thread) || !preview.expectedHash;
      entries.push({ thread: preview.thread, preview: blocked ? null : preview, state: blocked ? 'blocked' : 'ready', message: blocked ? preview.warning || (preview.targetExists ? '目标文件已存在，本次跳过。' : '当前没有可用的恢复条件，请重新扫描。') : null });
    } catch (error) {
      entries.push({ thread, preview: null, state: 'blocked', message: errorMessage(error) });
    }
  }
  return entries;
}

export async function restoreThreadBatch(api: ThreadsApi, entries: ThreadBatchEntry[], onProgress: (entry: ThreadBatchEntry) => void): Promise<void> {
  for (const entry of entries) {
    if (entry.state !== 'ready' || !entry.preview?.expectedHash) continue;
    onProgress({ ...entry, state: 'restoring' });
    try {
      await api.restore(entry.thread.key, entry.preview.expectedHash);
      onProgress({ ...entry, state: 'success', preview: null, message: null });
    } catch (error) {
      // Discard the previous authorization token after any failed write attempt.
      onProgress({ ...entry, state: 'failed', preview: null, message: errorMessage(error) });
    }
  }
}
