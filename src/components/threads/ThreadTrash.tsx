import { ChevronLeft, ChevronRight, LoaderCircle, RefreshCw, RotateCcw, Trash2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { threadsApi } from '../../lib/threads/api';
import { threadTime } from '../../lib/threads/presentation';
import type { ThreadTrashItem } from '../../lib/threads/types';
import { errorMessage } from '../../lib/utils';
import { ThreadDialog } from './ThreadDialog';

const PAGE_SIZE = 20;
export function ThreadTrash({ onClose }: { onClose: () => void }) {
  const client = useQueryClient();
  const [page, setPage] = useState(0);
  const [selected, setSelected] = useState<ThreadTrashItem | null>(null);
  const [notice, setNotice] = useState<string[] | null>(null);
  const list = useQuery({ queryKey: ['threads', 'trash', page], queryFn: () => threadsApi.trash({ offset: page * PAGE_SIZE, limit: PAGE_SIZE }), staleTime: 0, retry: false });
  const preview = useQuery({ queryKey: ['threads', 'trash-restore', selected?.id], queryFn: () => threadsApi.previewTrashRestore(selected!.id), enabled: Boolean(selected), staleTime: 0, retry: false });
  const restore = useMutation({
    mutationFn: ({ id, expectedHash }: { id: string; expectedHash: string }) => threadsApi.restoreTrash(id, expectedHash),
    onSuccess: (result) => {
      if (result.items.length > 0 && result.failedCount === 0 && result.items.every((item) => item.status === 'restored')) {
        setNotice([...new Set(result.items.map((item) => item.message).filter(Boolean))]);
        setSelected(null);
      }
      void client.invalidateQueries({ queryKey: ['threads'] });
    },
  });
  const pending = restore.isPending;
  const pages = Math.max(1, Math.ceil((list.data?.total || 0) / PAGE_SIZE));
  useEffect(() => { if (list.data && !list.isFetching && page >= pages) setPage(pages - 1); }, [list.data, list.isFetching, page, pages]);
  const failed = restore.data?.items.filter((item) => item.status !== 'restored');
  const needsRetry = Boolean(restore.error || preview.error || failed?.length || restore.data && (restore.data.failedCount > 0 || restore.data.items.length === 0));
  const error = selected ? preview.error || restore.error : list.error;
  function recheck() { restore.reset(); void preview.refetch(); }
  return <ThreadDialog title="线程回收站" description="保留删除记录的保护副本，可检查后撤销" onClose={onClose} locked={pending} wide>
    <div className="thread-dialog-body" tabIndex={0} aria-label="回收站记录与恢复结果">
      {notice && !selected && <div className="thread-restore-outcome" role="status"><strong>已撤销删除</strong>{notice.map((message) => <p key={message}>{message}</p>)}</div>}
      {selected ? <><div className="thread-trash-selected"><button className="text-button" disabled={pending} onClick={() => { setSelected(null); restore.reset(); }}><ChevronLeft size={14}/>返回回收站</button><h3>{selected.title || '未命名线程'}</h3><p>{selected.rolloutCount} 份记录文件 · 删除于 {threadTime(selected.deletedAt)}</p></div>{preview.isPending ? <p className="thread-loading-inline" role="status"><LoaderCircle className="spin" size={16}/>正在检查恢复条件</p> : preview.data && <><p className="thread-note">{preview.data.warning}</p>{preview.data.reason && <p className="thread-recovery-notice">{preview.data.reason}</p>}</>}{failed?.map((item) => <p className="inline-error" role="alert" key={item.key}>{item.message}</p>)}</> : list.isPending ? <div className="thread-loading" role="status"><LoaderCircle className="spin" size={24}/><span>正在读取回收站</span></div> : list.data?.items.length ? <><div className="thread-trash-list">{list.data.items.map((item) => <div className="thread-trash-row" key={item.id}><div><strong>{item.title || '未命名线程'}</strong><p>{item.rolloutCount} 份记录文件 · {threadTime(item.deletedAt)}</p>{item.state !== 'deleted' && <span className="thread-badge warning">需要检查</span>}{item.message && <small>{item.message}</small>}</div><button className="button button-quiet" onClick={() => { restore.reset(); setSelected(item); }}><RotateCcw size={14}/>预览撤销</button></div>)}</div><div className="thread-pagination"><span>{list.data.total} 条回收记录</span><div><button className="icon-button" aria-label="上一页回收站" disabled={page === 0 || list.isFetching} onClick={() => setPage(page - 1)}><ChevronLeft size={17}/></button><span>{Math.min(page + 1, pages)} / {pages}</span><button className="icon-button" aria-label="下一页回收站" disabled={page + 1 >= pages || list.isFetching} onClick={() => setPage(page + 1)}><ChevronRight size={17}/></button></div></div></> : !list.error && <div className="thread-empty"><span><Trash2 size={28}/></span><h3>回收站是空的</h3><p>在 AhaX 中删除的线程会出现在这里。</p></div>}
      {error && <p className="inline-error" role="alert">{errorMessage(error)}</p>}
      {!selected && list.error && <button className="button button-quiet" disabled={list.isFetching} onClick={() => void list.refetch()}>重新读取回收站</button>}
    </div>
    <footer className="thread-dialog-footer"><button className="button button-quiet" disabled={pending} onClick={onClose}>关闭</button>{selected && (needsRetry ? <button className="button button-primary" disabled={pending || preview.isFetching} onClick={recheck}><RefreshCw size={15}/>重新检查</button> : <button className="button button-primary" disabled={pending || !preview.data?.canRestore || preview.isFetching} onClick={() => { if (preview.data) restore.mutate({ id: selected.id, expectedHash: preview.data.expectedHash }); }}>{pending ? <LoaderCircle className="spin" size={16}/> : <RotateCcw size={16}/>}确认撤销删除</button>)}</footer>
  </ThreadDialog>;
}
