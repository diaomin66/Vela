import { ChevronRight, History, Trash2 } from 'lucide-react';
import { useState } from 'react';
import type { RunSummary } from '../../lib/evaluation';
import { readableTime } from '../../lib/utils';
import { EvaluationDialog } from './EvaluationDialog';
import { runLabels } from './presentation';

export function EvaluationHistory({ history, active, onSelect, onClose, onDelete, pending, scope }: { history: RunSummary[]; active: RunSummary | null; onSelect: (id: string) => void; onClose: () => void; onDelete: (ids: string[]) => void; pending: boolean; scope: 'manual' | 'scheduled' }) {
  const [selection, setSelection] = useState<Set<string>>(() => new Set());
  const records = active ? [active, ...history.filter((item) => item.id !== active.id)] : history;
  const available = history.filter((run) => run.status !== 'running').map((run) => run.id);
  const selected = available.filter((id) => selection.has(id));
  const all = available.length > 0 && selected.length === available.length;
  function toggle(id: string) { setSelection((previous) => { const next = new Set(previous); if (next.has(id)) next.delete(id); else next.add(id); return next; }); }
  return <EvaluationDialog title="评测记录" subtitle={`${scope === 'manual' ? '单次检测' : '定时评测'} · ${records.length} 轮记录`} onClose={onClose} locked={pending} className="evaluation-history-dialog">
    {!!available.length && <div className="evaluation-history-actions"><label><input type="checkbox" checked={all} disabled={pending} aria-label="选择全部评测记录" ref={(node) => { if (node) node.indeterminate = selected.length > 0 && !all; }} onChange={() => setSelection(new Set(all ? [] : available))}/>全选</label><span>{selected.length ? `已选 ${selected.length} 轮` : '可按轮次批量管理'}</span><button className="button button-quiet evaluation-delete-selected" disabled={!selected.length || pending} onClick={() => onDelete(selected)}><Trash2 size={15}/>删除所选{selected.length ? `（${selected.length}）` : ''}</button></div>}
    <div className="evaluation-report-scroll">{records.length ? <div className="evaluation-history-list">{records.map((run) => <div className="evaluation-history-item" key={run.id}>
      <input type="checkbox" aria-label={`选择 ${readableTime(run.startedAt)} 的评测`} checked={selected.includes(run.id)} disabled={run.status === 'running' || pending} onChange={() => toggle(run.id)}/>
      <button className="evaluation-history-row" onClick={() => onSelect(run.id)} disabled={pending}><span className={`evaluation-history-dot ${run.status}`}/><span><strong>{readableTime(run.startedAt)}</strong><small>{run.targetCount} 个模型 · {run.completedCases} / {run.totalCases} 项</small></span><span className="evaluation-history-state">{runLabels[run.status]}</span><ChevronRight size={16}/></button>
      <button className="icon-button evaluation-delete-record" aria-label={`删除 ${readableTime(run.startedAt)} 的评测`} disabled={run.status === 'running' || pending} onClick={() => onDelete([run.id])}><Trash2 size={15}/></button>
    </div>)}</div> : <div className="evaluation-report-empty"><History size={28}/><p>这里还没有评测记录。</p></div>}</div>
  </EvaluationDialog>;
}
