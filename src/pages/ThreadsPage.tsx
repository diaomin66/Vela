import { Archive, Check, ChevronLeft, ChevronRight, Folder, HardDrive, History, LoaderCircle, MessageSquareText, RefreshCw, Search, Settings2, ShieldCheck, X } from 'lucide-react';
import { useDeferredValue, useEffect, useMemo, useState } from 'react';
import { useThreads } from '../hooks/useThreads';
import { desktop } from '../lib/api';
import { canRestoreThread, formatThreadBytes, shortFolder, threadStatus, threadTime, threadTitle } from '../lib/threads/presentation';
import type { ThreadListQuery, ThreadSource, ThreadSummary } from '../lib/threads/types';
import { Select } from '../components/Select';
import { ThreadDetail } from '../components/threads/ThreadDetail';
import { ThreadRestore } from '../components/threads/ThreadRestore';
import { ThreadSettings } from '../components/threads/ThreadSettings';
import { ThreadReconcile, ThreadSources } from '../components/threads/ThreadSources';
import { ThreadDialog } from '../components/threads/ThreadDialog';
import { ThreadBatchRestore } from '../components/threads/ThreadBatchRestore';
import './threads.css';

const PAGE_SIZE = 20;
type Scope = 'all' | 'attention' | 'archived';
type Modal = { kind: 'detail' | 'restore'; thread: ThreadSummary } | { kind: 'settings' | 'sources' | 'rebuild' } | { kind: 'reconcile'; source: ThreadSource } | { kind: 'batch'; threads: ThreadSummary[] };

export function ThreadsPage() {
  const [scope, setScope] = useState<Scope>('all');
  const [search, setSearch] = useState('');
  const deferredSearch = useDeferredValue(search.trim().toLocaleLowerCase());
  const [source, setSource] = useState('all');
  const [page, setPage] = useState(0);
  const query = useMemo<ThreadListQuery>(() => ({ search: deferredSearch, scope: scope === 'archived' ? 'archived' : 'all', status: scope === 'attention' ? 'attention' : 'all', sourceId: source === 'all' ? null : source, offset: page * PAGE_SIZE, limit: PAGE_SIZE }), [deferredSearch, scope, source, page]);
  const threads = useThreads(query);
  const { data, settings, list } = threads;
  const [modal, setModal] = useState<Modal | null>(null);
  const [restored, setRestored] = useState(false);
  const [selected, setSelected] = useState<Map<string, ThreadSummary>>(() => new Map());
  const pageCount = Math.max(1, Math.ceil((list?.total ?? 0) / PAGE_SIZE));
  const currentPage = Math.min(page, pageCount - 1);
  const rows = list?.threads ?? [];
  const recoverableRows = rows.filter(canRestoreThread);
  const pageSelected = recoverableRows.length > 0 && recoverableRows.every((thread) => selected.has(thread.key));
  useEffect(() => { if (list && !threads.refreshingList && page >= pageCount) setPage(pageCount - 1); }, [list, page, pageCount, threads.refreshingList]);
  const open = (value: Modal) => { threads.resetError(); setModal(value); };
  async function restore(expectedHash: string) {
    if (modal?.kind !== 'restore') return;
    if (await threads.restore(modal.thread.key, expectedHash)) { removeSelected(modal.thread.key); setModal(null); setRestored(true); }
  }
  function switchScope(value: Scope) { setScope(value); setPage(0); }
  function selectThread(thread: ThreadSummary, checked: boolean) { setSelected((current) => { const next = new Map(current); if (checked) next.set(thread.key, thread); else next.delete(thread.key); return next; }); }
  function selectPage(checked: boolean) { setSelected((current) => { const next = new Map(current); for (const thread of recoverableRows) { if (checked) next.set(thread.key, thread); else next.delete(thread.key); } return next; }); }
  function removeSelected(key: string) { setSelected((current) => { const next = new Map(current); next.delete(key); return next; }); }
  return <div className="view-enter threads-page">
    <header className="page-heading thread-page-heading"><div><h1>线程管理</h1><p>找到旧对话，持续保护新的记录。</p></div><div className="thread-heading-actions"><button className="button button-quiet" disabled={!settings || threads.pending} onClick={() => open({ kind: 'settings' })}><Settings2 size={16}/><span>保护设置</span></button><button className="button button-primary" disabled={threads.pending || threads.scanning} onClick={() => { setRestored(false); void threads.scan(); }}>{threads.scanning ? <LoaderCircle className="spin" size={16}/> : <RefreshCw size={16}/>} {threads.scanning ? '正在扫描' : data?.scannedAt ? '重新扫描' : '扫描线程'}</button></div></header>
    {threads.error && !modal && <div className="inline-error" role="alert">{threads.error}<button className="text-button" onClick={() => void threads.refresh()}>重新读取</button></div>}
    {threads.dashboardError && !modal && <div className="thread-inventory-repair"><div><strong>保护清单无法读取</strong><p>可根据已保存的保护副本重建 AhaX 清单，原有清单会保留。</p></div><button className="button button-quiet" disabled={threads.pending} onClick={() => open({ kind: 'rebuild' })}>从保护副本重建清单</button></div>}
    {restored && <div className="thread-completed" role="status"><Check size={17}/><span>线程文件已恢复。重新打开 Codex 后检查线程列表。</span><button className="icon-button" aria-label="关闭恢复提示" onClick={() => setRestored(false)}><X size={15}/></button></div>}
    {!data ? !threads.dashboardError && <div className="thread-loading" role="status"><LoaderCircle className="spin" size={27}/><span>正在读取线程目录</span></div> : <>
      <section className="thread-protection" aria-label="线程保护状态"><div className="thread-protection-intro"><span className="thread-protection-mark"><ShieldCheck size={27}/></span><div><h2>{threads.scanning ? '正在检查线程' : settings?.enabled ? '自动保护已开启' : '自动保护已暂停'}</h2><p>{data.protection.lastSuccessAt ? `最近保护 ${threadTime(data.protection.lastSuccessAt)}` : '完成首次扫描后开始建立保护副本'}{data.protection.bytesProtected > 0 && ` · ${formatThreadBytes(data.protection.bytesProtected)}`}</p></div></div><dl className="thread-protection-counts"><div><dt>全部线程</dt><dd>{data.total.toLocaleString()}</dd></div><div><dt>已有备份</dt><dd>{data.protected.toLocaleString()}</dd></div><div><dt>可找回</dt><dd>{data.recoverable.toLocaleString()}</dd></div></dl><button className="thread-sources-button" onClick={() => open({ kind: 'sources' })}><HardDrive size={15}/>{data.sources.length} 个数据来源<ChevronRight size={14}/></button></section>
      {(data.error || data.protection.error) && <p className="inline-error" role="alert">{data.error || data.protection.error}</p>}
      <div className="thread-library-toolbar"><nav className="thread-scopes" aria-label="线程筛选">{([{ value: 'all', label: '全部', count: data.total }, { value: 'attention', label: '待处理', count: data.attention }, { value: 'archived', label: '已归档', count: null }] as const).map((item) => <button key={item.value} aria-current={scope === item.value ? 'page' : undefined} onClick={() => switchScope(item.value)}>{item.label}{item.count !== null && <span>{item.count}</span>}</button>)}</nav><div className="thread-search-controls"><label className="search-input"><Search size={16}/><input aria-label="搜索线程" placeholder="搜索名称、项目或线程 ID" value={search} onChange={(event) => { setSearch(event.target.value); setPage(0); }}/>{search && <button className="icon-button" aria-label="清除线程搜索" onClick={() => { setSearch(''); setPage(0); }}><X size={14}/></button>}</label>{data.sources.length > 1 && <Select ariaLabel="筛选数据来源" value={source} onChange={(value) => { setSource(value); setPage(0); }} options={[{ value: 'all', label: '全部来源' }, ...data.sources.map((item) => ({ value: item.id, label: item.displayRoot || '数据目录' }))]}/>}</div></div>
      <section className="thread-library" aria-label="线程列表" aria-busy={threads.scanning || threads.refreshingList || search.trim().toLocaleLowerCase() !== deferredSearch}>
        {threads.loadingList ? <div className="thread-loading" role="status"><LoaderCircle className="spin" size={24}/><span>正在读取线程</span></div> : rows.length ? <>{recoverableRows.length > 0 && <div className="thread-selection-tools"><label><input type="checkbox" checked={pageSelected} disabled={threads.refreshingList || threads.pending} onChange={(event) => selectPage(event.target.checked)}/>选择本页可找回</label><span>{recoverableRows.length} 条</span></div>}<div className="thread-list-heading"><span>线程与项目</span><span>状态</span><span>最后更新</span><span/></div><div className="thread-rows">{rows.map((thread) => <div className={`thread-record${selected.has(thread.key) ? ' is-selected' : ''}`} key={thread.key}><span className="thread-row-choice">{canRestoreThread(thread) && <input type="checkbox" aria-label={`选择找回 ${threadTitle(thread)}`} checked={selected.has(thread.key)} disabled={threads.refreshingList || threads.pending} onChange={(event) => selectThread(thread, event.target.checked)}/>}</span><ThreadRow thread={thread} onSelect={() => open({ kind: 'detail', thread })}/></div>)}</div><footer className="thread-pagination"><span>{(list?.total ?? 0).toLocaleString()} 条记录 · 按更新时间排序</span><div><button className="icon-button" aria-label="上一页线程" disabled={currentPage === 0 || threads.refreshingList} onClick={() => setPage(currentPage - 1)}><ChevronLeft size={17}/></button><span>{currentPage + 1} / {pageCount}</span><button className="icon-button" aria-label="下一页线程" disabled={currentPage + 1 >= pageCount || threads.refreshingList} onClick={() => setPage(currentPage + 1)}><ChevronRight size={17}/></button></div></footer></> : <div className="thread-empty"><span><History size={28}/></span><h2>{data.total ? '没有符合条件的线程' : '从现有记录开始保护'}</h2><p>{data.total ? '试着调整筛选条件，或搜索其他名称。' : '扫描当前数据目录，识别活动记录和已归档线程。'}</p>{data.total ? <button className="button button-quiet" onClick={() => { setSearch(''); setSource('all'); switchScope('all'); }}>清除筛选</button> : <button className="button button-primary" disabled={threads.pending} onClick={() => void threads.scan()}><RefreshCw size={15}/>开始扫描</button>}</div>}
      </section>
      {selected.size > 0 && <div className="thread-batch-actions"><span>已选择 <strong>{selected.size}</strong> 条</span><button className="text-button" disabled={threads.pending} onClick={() => setSelected(new Map())}>清空</button><button className="button button-primary" disabled={threads.pending} onClick={() => open({ kind: 'batch', threads: [...selected.values()] })}>预览找回 {selected.size} 条</button></div>}
      <p className="thread-page-note">{desktop ? '保护副本保存在本机。文件已彻底删除且没有备份时，无法仅靠索引恢复内容。' : '演示数据 · 不读取或修改本机线程'}</p>
      {modal?.kind === 'detail' && <ThreadDetail key={modal.thread.key} thread={modal.thread} source={data.sources.find((item) => item.id === modal.thread.sourceId)} onClose={() => setModal(null)} onRestore={(thread) => open({ kind: 'restore', thread })}/>}
      {modal?.kind === 'restore' && <ThreadRestore key={modal.thread.key} thread={modal.thread} pending={threads.pending} error={threads.error} onClose={() => setModal(null)} onConfirm={(hash) => void restore(hash)}/>}
      {modal?.kind === 'settings' && settings && <ThreadSettings initial={settings} pending={threads.pending} error={threads.error} onClose={() => setModal(null)} onSave={threads.saveSettings}/>}
      {modal?.kind === 'sources' && <ThreadSources sources={data.sources} onClose={() => setModal(null)} onReconcile={(value) => open({ kind: 'reconcile', source: value })}/>}
      {modal?.kind === 'reconcile' && <ThreadReconcile source={modal.source} onClose={() => setModal(null)} onComplete={threads.refresh}/>}
      {modal?.kind === 'batch' && <ThreadBatchRestore threads={modal.threads} onClose={() => setModal(null)} onRecovered={removeSelected}/>}
    </>}
    {modal?.kind === 'rebuild' && <ThreadDialog title="重建保护清单" description="根据已保存的保护副本恢复 AhaX 记录" onClose={() => setModal(null)} locked={threads.pending}><div className="thread-dialog-body"><p className="thread-note">旧清单会保留，再从保护副本重新整理记录。此操作不会修改 Codex 的原始线程或替代官方列表索引。</p>{threads.error && <p className="inline-error" role="alert">{threads.error}</p>}</div><footer className="thread-dialog-footer"><button className="button button-quiet" disabled={threads.pending} onClick={() => setModal(null)}>取消</button><button className="button button-primary" disabled={threads.pending} onClick={() => void threads.rebuild().then((complete) => { if (complete) setModal(null); })}>{threads.pending ? <LoaderCircle className="spin" size={16}/> : <RefreshCw size={16}/>}重建清单</button></footer></ThreadDialog>}
  </div>;
}

function ThreadRow({ thread, onSelect }: { thread: ThreadSummary; onSelect: () => void }) {
  const status = threadStatus(thread);
  return <button className="thread-row" onClick={onSelect} aria-label={`查看线程 ${threadTitle(thread)}`}><span className="thread-row-main"><span className="thread-row-mark">{thread.archived ? <Archive size={18}/> : <MessageSquareText size={18}/>}</span><span><strong>{threadTitle(thread)}</strong><span className="thread-row-subtitle"><Folder size={12}/>{shortFolder(thread.cwd)}<i/> {thread.provider || '服务商未记录'}{thread.selectedRollout === false && <><i/>历史版本</>}</span></span></span><span className={`thread-badge ${status.tone}`}>{status.label}</span><time className="thread-row-time" dateTime={thread.updatedAt ?? undefined}>{threadTime(thread.updatedAt)}</time><ChevronRight className="thread-row-chevron" size={15}/></button>;
}
