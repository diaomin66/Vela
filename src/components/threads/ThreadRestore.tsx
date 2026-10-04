import { ArrowDownToLine, CircleAlert, FileCheck2, LoaderCircle, RotateCcw } from 'lucide-react';
import { useThreadRestore } from '../../hooks/useThreads';
import { formatThreadBytes, threadTitle } from '../../lib/threads/presentation';
import type { ThreadSummary } from '../../lib/threads/types';
import { errorMessage } from '../../lib/utils';
import { ThreadDialog } from './ThreadDialog';

export function ThreadRestore({ thread, pending, error, onClose, onConfirm }: { thread: ThreadSummary; pending: boolean; error: string | null; onClose: () => void; onConfirm: (expectedHash: string) => void }) {
  const query = useThreadRestore(thread.key);
  const value = query.data;
  const conflicting = value?.conflict || value?.targetExists && value.targetHash !== value.snapshotHash;
  return <ThreadDialog title="恢复线程" description="检查保护副本与目标位置后再恢复" onClose={onClose} locked={pending}>
    <div className="thread-dialog-body">
      <div className="thread-restore-summary"><span className="thread-detail-mark"><RotateCcw size={22}/></span><div><h3>{threadTitle(thread)}</h3><p>{formatThreadBytes(thread.bytes)} · 保留原线程标识</p></div></div>
      {query.isPending && <div className="thread-loading" role="status"><LoaderCircle className="spin" size={24}/><span>正在核对快照</span></div>}
      {query.error && <div className="inline-error" role="alert">{errorMessage(query.error)}<button className="text-button" onClick={() => void query.refetch()}>重新检查</button></div>}
      {value && <><div className="thread-restore-check"><FileCheck2 size={18}/><span>保护副本已找到</span><strong>{value.targetExists ? conflicting ? '目标存在不同内容' : '目标已存在相同内容' : '目标位置可恢复'}</strong></div><div className="thread-restore-destination"><span><ArrowDownToLine size={15}/>恢复至</span><code>{value.targetPath}</code></div>{(value.warning || conflicting) && <p className="thread-recovery-notice"><CircleAlert size={17}/><span>{value.warning || '目标文件已有不同内容。请先处理冲突，本次不会覆盖。'}</span></p>}<p className="thread-note">恢复前会重新核对文件状态。恢复后重新打开 Codex，检查线程是否正常显示。</p></>}
      {error && <div className="inline-error" role="alert">{error}<button className="text-button" disabled={pending} onClick={() => void query.refetch()}>重新预览</button></div>}
    </div>
    <footer className="thread-dialog-footer"><button className="button button-quiet" disabled={pending} onClick={onClose}>取消</button><button className="button button-primary" disabled={pending || !value || query.isFetching || !!conflicting || !!query.error} onClick={() => value && onConfirm(value.expectedHash)}>{pending ? <LoaderCircle className="spin" size={16}/> : <RotateCcw size={16}/>} {pending ? '正在恢复' : '确认恢复'}</button></footer>
  </ThreadDialog>;
}
