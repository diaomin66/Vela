import { Archive, ArrowUpRight, Check, Copy, FileText, Folder, HardDrive, LoaderCircle, RotateCcw, ShieldCheck } from 'lucide-react';
import { useMutation } from '@tanstack/react-query';
import { useState } from 'react';
import { useThreadDetail } from '../../hooks/useThreads';
import { canRestoreThread, formatThreadBytes, integrityLabel, threadDependencyIssue, threadStatus, threadTime, threadTitle } from '../../lib/threads/presentation';
import type { ThreadSource, ThreadSummary } from '../../lib/threads/types';
import { errorMessage } from '../../lib/utils';
import { threadsApi } from '../../lib/threads/api';
import { ThreadDialog } from './ThreadDialog';

export function ThreadDetail({ thread, source, onClose, onRestore }: { thread: ThreadSummary; source?: ThreadSource; onClose: () => void; onRestore: (thread: ThreadSummary) => void }) {
  const query = useThreadDetail(thread.key);
  const value = query.data?.summary ?? thread;
  const status = threadStatus(value);
  const dependency = threadDependencyIssue(value);
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState(false);
  const opening = useMutation({ mutationFn: () => threadsApi.open(value.key) });
  async function copyId() {
    try { await navigator.clipboard.writeText(value.threadId); setCopied(true); setCopyError(false); }
    catch { setCopyError(true); }
  }
  return <ThreadDialog title="线程详情" description="核对来源、原文件与备份状态" wide onClose={onClose}>
    <div className="thread-dialog-body">
      <div className="thread-detail-title"><span className="thread-detail-mark">{value.archived ? <Archive size={23}/> : <FileText size={23}/>}</span><div><h3>{threadTitle(value)}</h3><span className={`thread-badge ${status.tone}`}>{status.label}</span>{value.archived && <span className="thread-badge neutral">已归档</span>}{value.selectedRollout === false && <span className="thread-badge neutral">历史版本</span>}</div></div>
      {query.isPending && <p className="thread-loading-inline" role="status"><LoaderCircle className="spin" size={16}/>正在核对文件</p>}
      {query.error && <div className="inline-error" role="alert">{errorMessage(query.error)}<button className="text-button" onClick={() => void query.refetch()}>重试</button></div>}
      {opening.error && <div className="inline-error" role="alert">{errorMessage(opening.error)}</div>}
      {opening.data && <p className="thread-opened-note" role="status">{opening.data}</p>}
      <div className="thread-health-pair"><div><FileText size={17}/><span>原始记录</span><strong>{integrityLabel(value.integrity)}</strong></div><div><ShieldCheck size={17}/><span>保护副本</span><strong>{query.data?.snapshotHash ? `已保存 · ${formatThreadBytes(query.data.snapshotBytes ?? 0)}` : value.snapshot === 'pending' ? '等待文件稳定后备份' : value.snapshot === 'protected' ? '已有备份，正在核对' : '尚无可用快照'}</strong></div></div>
      {canRestoreThread(value) && <div className="thread-recovery-notice"><RotateCcw size={18}/><p>原文件已缺失，找到一份保护副本。可先查看恢复位置与冲突检查。</p></div>}
      {dependency && <div className="thread-recovery-notice"><FileText size={18}/><p>{dependency === 'cycle' ? '这条记录的历史引用形成循环，暂不能确认完整对话。请先从可信备份找回正确的历史基础记录，再重新扫描。' : '这条记录依赖的历史基础尚不完整。请先找回缺失的历史基础记录，再重新扫描；仅恢复当前文件无法找回完整对话。'}</p></div>}
      {value.integrity === 'missing' && !canRestoreThread(value) && !dependency && <div className="thread-recovery-notice"><FileText size={18}/><p>尚未找到可用副本。可重新扫描已识别的数据目录，或检查其他设备与系统备份。</p></div>}
      {value.stateIndex === 'missing' && value.integrity !== 'missing' && <div className="thread-recovery-notice"><FileText size={18}/><p>原始记录仍在，当前列表索引尚未收录。可在「数据来源」中为此目录重新索引。</p></div>}
      <dl className="thread-metadata">
        <div><dt><Folder size={15}/>工作目录</dt><dd>{value.cwd || '未记录'}</dd></div>
        <div><dt><HardDrive size={15}/>数据来源</dt><dd>{source?.displayRoot ?? '已识别数据目录'}</dd></div>
        <div><dt>服务商</dt><dd>{value.provider || '旧记录未提供'}</dd></div>
        <div><dt>最后更新</dt><dd>{threadTime(value.updatedAt)}</dd></div>
        <div><dt>创建时间</dt><dd>{threadTime(value.createdAt)}</dd></div>
        <div><dt>记录体积</dt><dd>{formatThreadBytes(value.bytes)} · {value.lineCount.toLocaleString()} 行</dd></div>
        {value.stateIndex && <div><dt>列表索引</dt><dd>{value.stateIndex === 'indexed' ? '已收录' : value.stateIndex === 'missing' ? '尚未收录' : '暂时无法核对'}</dd></div>}
      </dl>
      <details className="thread-technical"><summary>文件与标识</summary><dl className="thread-metadata"><div><dt>线程 ID</dt><dd className="thread-copy-value"><code>{value.threadId}</code><button className="icon-button" aria-label={copied ? '已复制线程 ID' : '复制线程 ID'} onClick={() => void copyId()}>{copied ? <Check size={15}/> : <Copy size={15}/>}</button></dd></div>{value.historyBase && <div><dt>历史基础 ID</dt><dd><code>{value.historyBase.threadId}</code><p className="thread-note">对应基础记录文件的标识，可与备份中的文件名核对。</p></dd></div>}<div><dt>记录文件</dt><dd><code>{value.path}</code></dd></div><div><dt>名称索引</dt><dd>{value.indexPresent ? '已记录名称' : '未记录名称，不代表线程丢失'}</dd></div></dl>{copyError && <p className="thread-note" role="status">无法访问剪贴板，可选中上方 ID 手动复制。</p>}</details>
    </div>
    <footer className="thread-dialog-footer"><button className="button button-quiet" onClick={onClose}>关闭</button>{query.data?.rawAvailable && ['valid', 'healthy'].includes(value.integrity) && !dependency && <button className="button button-primary" disabled={opening.isPending} onClick={() => opening.mutate()}>{opening.isPending ? <LoaderCircle className="spin" size={15}/> : <ArrowUpRight size={16}/>}在 Codex 打开</button>}{canRestoreThread(value) && <button className="button button-primary" onClick={() => onRestore(value)}><RotateCcw size={15}/>预览恢复</button>}</footer>
  </ThreadDialog>;
}
