import { Check, CircleAlert, LoaderCircle, RotateCcw } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { threadKeys } from '../../hooks/useThreads';
import { threadsApi } from '../../lib/threads/api';
import { previewThreadBatch, restoreThreadBatch, type ThreadBatchEntry } from '../../lib/threads/batch';
import { shortFolder, threadTitle } from '../../lib/threads/presentation';
import type { ThreadSummary } from '../../lib/threads/types';
import { ThreadDialog } from './ThreadDialog';

export function ThreadBatchRestore({ threads, onClose, onRecovered }: { threads: ThreadSummary[]; onClose: () => void; onRecovered: (key: string) => void }) {
  const client = useQueryClient();
  const initial = useRef(threads);
  const started = useRef(false);
  const locked = useRef(false);
  const [entries, setEntries] = useState<ThreadBatchEntry[]>([]);
  const [attempted, setAttempted] = useState(false);
  const preview = useMutation({
    mutationFn: (candidates: ThreadSummary[]) => previewThreadBatch(threadsApi, candidates),
    onSuccess: (result) => setEntries((current) => [...current.filter((entry) => entry.state === 'success'), ...result]),
  });
  const restore = useMutation({
    mutationFn: async (prepared: ThreadBatchEntry[]) => {
      await client.cancelQueries({ queryKey: threadKeys.all });
      await restoreThreadBatch(threadsApi, prepared, (entry) => {
        setEntries((current) => current.map((item) => item.thread.key === entry.thread.key ? entry : item));
        if (entry.state === 'success') onRecovered(entry.thread.key);
      });
    },
    onSettled: () => { locked.current = false; void client.invalidateQueries({ queryKey: threadKeys.all }); },
  });
  const startPreview = preview.mutate;
  useEffect(() => { if (!started.current) { started.current = true; startPreview(initial.current); } }, [startPreview]);
  const busy = preview.isPending || restore.isPending;
  const ready = entries.filter((entry) => entry.state === 'ready').length;
  const successes = entries.filter((entry) => entry.state === 'success').length;
  const remaining = entries.filter((entry) => entry.state !== 'success' && entry.state !== 'restoring');
  const blocked = entries.filter((entry) => entry.state === 'blocked' || entry.state === 'failed').length;
  function confirm() {
    if (busy || locked.current || !ready) return;
    locked.current = true;
    setAttempted(true);
    restore.mutate(entries);
  }
  return <ThreadDialog title="批量找回线程" description="逐条核对保护副本，只恢复预览中可用的记录" onClose={onClose} locked={busy} wide>
    <div className="thread-dialog-body">
      {preview.isPending ? <div className="thread-loading" role="status"><LoaderCircle className="spin" size={24}/><span>正在逐条核对 {remaining.length || initial.current.length} 条记录</span></div> : <>
        <div className="thread-batch-summary" role="status">{restore.isPending ? <><LoaderCircle className="spin" size={17}/><span>正在恢复 · 已完成 {successes} 条</span></> : attempted ? <><Check size={17}/><span>已找回 {successes} 条{blocked > 0 ? ` · ${blocked} 条待处理` : ''}</span></> : <><RotateCcw size={17}/><span>可恢复 {ready} 条{blocked > 0 ? ` · ${blocked} 条暂不可用` : ''}</span></>}</div>
        <div className="thread-batch-list">{entries.map((entry) => <div className="thread-batch-entry" key={entry.thread.key} data-state={entry.state}><div className="thread-batch-entry-heading"><strong>{threadTitle(entry.thread)}</strong><span className={`thread-badge ${entry.state === 'success' ? 'success' : ['blocked', 'failed'].includes(entry.state) ? 'warning' : 'neutral'}`}>{({ ready: '可恢复', blocked: '暂不可用', restoring: '恢复中', success: '已找回', failed: '恢复失败' })[entry.state]}</span></div><p>{shortFolder(entry.thread.cwd)}</p>{entry.message && <p className="thread-batch-error"><CircleAlert size={13}/><span>{entry.message}</span></p>}{entry.preview && <details><summary>查看恢复位置</summary><code>{entry.preview.targetPath}</code></details>}</div>)}</div>
        <p className="thread-note thread-batch-note">{attempted ? '已完成的记录已移出选择，未成功的记录会保留。找回后重新打开 Codex 检查列表。' : '确认后依次恢复；目标有冲突或副本不可用的记录会跳过。'}</p>
      </>}
    </div>
    <footer className="thread-dialog-footer"><button className="button button-quiet" disabled={busy} onClick={onClose}>{attempted ? '完成' : '取消'}</button>{ready > 0 && !preview.isPending ? <button className="button button-primary" disabled={busy} onClick={confirm}>{restore.isPending ? <LoaderCircle className="spin" size={16}/> : <RotateCcw size={16}/>}确认找回 {ready} 条</button> : !busy && remaining.length > 0 && <button className="button button-primary" onClick={() => { setAttempted(false); preview.mutate(remaining.map((entry) => entry.thread)); }}><RotateCcw size={16}/>重新预览 {remaining.length} 条</button>}</footer>
  </ThreadDialog>;
}
