import { CircleAlert, LoaderCircle, RefreshCw, Trash2 } from 'lucide-react';
import { useEffect, useRef } from 'react';
import { useThreadDeletion } from '../../hooks/useThreads';
import type { ThreadSummary } from '../../lib/threads/types';
import { errorMessage } from '../../lib/utils';
import { ThreadDialog } from './ThreadDialog';

export function ThreadDelete({ threads, onClose, onRemovedFromList, onOpenTrash }: { threads: ThreadSummary[]; onClose: () => void; onRemovedFromList: (keys: string[]) => void; onOpenTrash: () => void }) {
  const { preview, remove } = useThreadDeletion();
  const remaining = useRef(threads.map((thread) => thread.key));
  const started = useRef(false);
  const inspect = preview.mutate;
  useEffect(() => { if (!started.current) { started.current = true; inspect(remaining.current); } }, [inspect]);
  const value = preview.data;
  const pending = preview.isPending || remove.isPending;
  const canDelete = value?.items.filter((item) => item.canDelete).length || 0;
  const blocked = value?.items.filter((item) => !item.canDelete).length || 0;
  const interrupted = remove.data?.items.filter((item) => item.status === 'interrupted').length || 0;
  const needsRetry = Boolean(preview.error || remove.error || remove.data?.items.some((item) => item.status === 'failed' || item.status === 'blocked'));
  function retry() { remove.reset(); inspect(remaining.current); }
  async function confirm() {
    if (!value || !canDelete || pending || remove.data || remove.error) return;
    try {
      const result = await remove.mutateAsync({ keys: remaining.current, expectedHash: value.expectedHash });
      // Interrupted operations already have a durable tombstone. Check them in
      // the recycle bin instead of retrying a now-hidden inventory selection.
      const removed = result.items.filter((item) => item.status === 'deleted' || item.status === 'interrupted' && item.trashId).flatMap((item) => {
        const group = value.items.find((entry) => entry.key === item.key);
        return group ? threads.filter((thread) => thread.sourceId === group.sourceId && thread.threadId === group.threadId).map((thread) => thread.key) : [];
      });
      const removedKeys = new Set(removed);
      remaining.current = remaining.current.filter((key) => !removedKeys.has(key));
      onRemovedFromList(removed);
    } catch { /* The mutation error stays visible until a fresh preview. */ }
  }
  return <ThreadDialog title="删除线程" description="核对影响范围后移入 ahaX 回收站" onClose={onClose} locked={pending} wide>
    <div className="thread-dialog-body" tabIndex={0} aria-label="线程删除检查结果">
      {preview.isPending && <div className="thread-loading" role="status"><LoaderCircle className="spin" size={24}/><span>正在检查线程与依赖关系</span></div>}
      {value && <><div className="thread-delete-intro" role="status"><Trash2 size={22}/><div><h3>{remove.data ? `已移入回收站 ${remove.data.deletedCount} 条` : `已选择 ${value.logicalCount} 条线程`}</h3><p>{remove.data ? `${remove.data.items.filter((item) => item.status !== 'deleted').length} 条待处理` : `${value.rolloutCount} 份记录文件${blocked ? ` · ${blocked} 条暂不能删除` : ''}`}</p></div></div><p className="thread-note">{value.warning}</p><div className="thread-delete-list">{value.items.map((item) => {
        const outcome = remove.data?.items.find((result) => result.key === item.key);
        const state = outcome?.status || (item.canDelete ? 'ready' : 'blocked');
        return <div className="thread-delete-item" key={item.key} data-state={state}><div><strong>{item.title || '未命名线程'}</strong><span className={`thread-badge ${state === 'deleted' ? 'success' : state === 'ready' ? 'neutral' : 'warning'}`}>{state === 'deleted' ? '已删除' : state === 'failed' ? '删除失败' : state === 'blocked' ? '暂不能删除' : state === 'ready' ? '可删除' : '需要确认'}</span></div><p>{item.rolloutCount} 份记录文件</p>{(outcome?.message || item.reason) && <p className="thread-delete-reason"><CircleAlert size={13}/>{outcome?.message || item.reason}</p>}</div>;
      })}</div></>}
      {(preview.error || remove.error) && <p className="inline-error" role="alert">{errorMessage(preview.error || remove.error)}</p>}
    </div>
    <footer className="thread-dialog-footer"><button className="button button-quiet" disabled={pending} onClick={onClose}>{remove.data ? '完成' : '取消'}</button>{needsRetry && <button className="button button-primary" disabled={pending || !remaining.current.length} onClick={retry}><RefreshCw size={15}/>重新检查</button>}{interrupted > 0 && <button className="button button-primary" disabled={pending} onClick={onOpenTrash}><Trash2 size={15}/>前往回收站</button>}{!remove.data && !needsRetry && <button className="button button-danger" disabled={pending || !value || !canDelete} onClick={() => void confirm()}>{remove.isPending ? <LoaderCircle className="spin" size={16}/> : <Trash2 size={16}/>}确认删除 {canDelete} 条</button>}</footer>
  </ThreadDialog>;
}
