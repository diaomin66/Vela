import { Check, Database, Folder, LoaderCircle, RefreshCw } from 'lucide-react';
import { useMutation } from '@tanstack/react-query';
import type { ThreadSource } from '../../lib/threads/types';
import { threadsApi } from '../../lib/threads/api';
import { errorMessage } from '../../lib/utils';
import { ThreadDialog } from './ThreadDialog';

export function ThreadSources({ sources, onClose, onReconcile }: { sources: ThreadSource[]; onClose: () => void; onReconcile: (source: ThreadSource) => void }) {
  return <ThreadDialog title="数据来源" description="当前配置目录与已识别的历史记录位置" wide onClose={onClose}>
    <div className="thread-dialog-body thread-sources">{sources.map((source) => <section className="thread-source" key={source.id}><div className="thread-source-heading"><span className="thread-detail-mark"><Folder size={21}/></span><div><h3>{source.displayRoot || '线程数据目录'}</h3><span>{source.available ? source.writable ? '可读写' : '只读' : '暂不可用'}</span></div></div><code>{source.root}</code>{source.error && <p className="inline-error" role="alert">{source.error}</p>}<div className="thread-source-footer"><p>原记录仍在，列表里找不到时可重新索引。</p><button className="button button-quiet" disabled={!source.available || !source.writable} onClick={() => onReconcile(source)}><RefreshCw size={14}/>重新索引</button></div></section>)}{!sources.length && <p className="thread-note">尚未识别到数据目录，请先扫描。</p>}</div>
    <footer className="thread-dialog-footer"><button className="button button-quiet" onClick={onClose}>关闭</button></footer>
  </ThreadDialog>;
}

export function ThreadReconcile({ source, onClose, onComplete }: { source: ThreadSource; onClose: () => void; onComplete: () => Promise<void> }) {
  const mutation = useMutation({ mutationFn: () => threadsApi.reconcile(source.id), onSuccess: () => { void onComplete(); } });
  return <ThreadDialog title="重新索引线程" description={source.displayRoot || '刷新本机线程列表'} onClose={onClose} locked={mutation.isPending}>
    <div className="thread-dialog-body"><div className="thread-restore-summary"><span className="thread-detail-mark">{mutation.isSuccess ? <Check size={24}/> : <Database size={24}/>}</span><div><h3>{mutation.isSuccess ? '索引检查完成' : '找回列表中未显示的线程'}</h3><p>{mutation.isSuccess ? '重新打开客户端后检查显示结果' : '适用于原始记录文件仍然存在的情况'}</p></div></div>
      {mutation.data ? <><div className="thread-reconcile-counts"><div><strong>{mutation.data.activeCount}</strong><span>活动线程</span></div><div><strong>{mutation.data.archivedCount}</strong><span>已归档</span></div></div><p className="thread-note" role="status">{mutation.data.message}</p></> : <><p className="thread-note">先完成线程与索引快照，再由本机官方服务按当前版本刷新列表。不会发送模型请求。完成后重新打开客户端。</p><code className="thread-source-path">{source.root}</code></>}
      {mutation.error && <p className="inline-error" role="alert">{errorMessage(mutation.error)}</p>}
    </div>
    <footer className="thread-dialog-footer"><button className="button button-quiet" disabled={mutation.isPending} onClick={onClose}>{mutation.isSuccess ? '完成' : '取消'}</button>{!mutation.isSuccess && <button className="button button-primary" disabled={mutation.isPending} onClick={() => mutation.mutate()}>{mutation.isPending ? <LoaderCircle className="spin" size={16}/> : <RefreshCw size={16}/>} {mutation.isPending ? '正在保护并刷新' : mutation.isError ? '重试' : '开始重新索引'}</button>}</footer>
  </ThreadDialog>;
}
