import * as Tooltip from '@radix-ui/react-tooltip';
import { Candy, CheckCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import type { EvaluationCaseId } from '../../lib/evaluation';
import { EvaluationDialog } from './EvaluationDialog';
import { caseTitles, duration, effortName, recordKey, recordTime, resultLabels, type EvaluationRecord } from './presentation';

const HALF_HOUR = 30 * 60 * 1000;
const states = ['passed', 'failed', 'error', 'cancelled', 'empty'] as const;
const stateLabels = { passed: '通过', failed: '未通过', error: '请求失败', cancelled: '已取消', empty: '未检测' };
type SlotState = typeof states[number];
function slotState(records: EvaluationRecord[]): SlotState { return records.some((record) => record.status === 'error') ? 'error' : records.some((record) => record.status === 'failed') ? 'failed' : records.some((record) => record.status === 'passed') ? 'passed' : records.length ? 'cancelled' : 'empty'; }

export function EvaluationTimeline({ records, caseId, onSelect }: { records: EvaluationRecord[]; caseId: Exclude<EvaluationCaseId, 'pelican'>; onSelect: (record: EvaluationRecord) => void }) {
  const [now, setNow] = useState(Date.now);
  const [selectedSlot, setSelectedSlot] = useState<EvaluationRecord[] | null>(null);
  useEffect(() => { const timer = setInterval(() => setNow(Date.now()), 60000); return () => clearInterval(timer); }, []);
  const start = now - 48 * HALF_HOUR;
  const groups = new Map<string, EvaluationRecord[]>();
  for (const record of records) { const key = recordKey(record); const group = groups.get(key); if (group) group.push(record); else groups.set(key, [record]); }
  const Icon = caseId === 'candy' ? Candy : CheckCheck;
  return <Tooltip.Provider delayDuration={200}><div className="evaluation-timelines" data-testid={`${caseId}-timeline`}>
    <header className="evaluation-timeline-overview"><div><h2>24 小时检测时间线</h2><p>每个色块 30 分钟，点击查看当次回答。</p></div><div className="evaluation-timeline-legend">{states.map((state) => <span key={state}><i className={state}/>{stateLabels[state]}</span>)}</div></header>
    {[...groups].map(([key, entries]) => {
      const latest = entries[0];
      const slots: EvaluationRecord[][] = Array.from({ length: 48 }, () => []);
      for (const record of entries) { const index = Math.floor((new Date(record.createdAt).getTime() - start) / HALF_HOUR); if (index >= 0 && index < 48) slots[index].push(record); }
      const recent = slots.flat();
      const passed = recent.filter((record) => record.status === 'passed').length;
      return <section className="evaluation-timeline-card" key={key} aria-label={`${latest.channelName} ${latest.modelId} ${caseTitles[caseId]} 时间线`}>
        <header className="evaluation-timeline-heading"><span className="evaluation-channel-mark"><Icon size={20}/></span><div className="evaluation-timeline-model"><h3>{latest.modelAlias || latest.modelId}</h3>{latest.modelAlias && latest.modelAlias !== latest.modelId && <p>{latest.modelId}</p>}<p>{latest.channelName} · {effortName(latest.reasoningEffort)}推理</p><p>最近检测 {recordTime(latest.createdAt)}</p></div><div className="evaluation-timeline-rate"><span>24 小时通过率</span><strong>{recent.length ? Math.round(passed / recent.length * 100) : '—'}<small>{recent.length ? '%' : ''}</small></strong><span>{recent.length ? `${passed} / ${recent.length} 次通过` : '暂无检测'}</span></div></header>
        <div className="evaluation-timeline-blocks" role="group" aria-label="最近 24 小时，每格 30 分钟">{slots.map((slot, index) => {
          const state = slotState(slot);
          const from = recordTime(new Date(start + index * HALF_HOUR).toISOString());
          const chosen = slot.find((record) => record.status === 'error' || record.status === 'failed') ?? slot[0];
          return <Tooltip.Root key={index}><Tooltip.Trigger asChild><button className={`evaluation-time-block ${state}`} data-state-value={state} aria-label={`${from}，${stateLabels[state]}${slot.length ? `，${slot.length} 次检测` : ''}`} aria-disabled={!slot.length} onClick={() => { if (slot.length > 1) setSelectedSlot(slot); else if (chosen) onSelect(chosen); }}/></Tooltip.Trigger><Tooltip.Portal><Tooltip.Content className="evaluation-timeline-tooltip" sideOffset={8}><strong>{from}</strong>{slot.length ? <><span>{slot.length} 次检测 · {stateLabels[state]}</span><span>{chosen.modelId} · {duration(chosen.elapsedMs)}</span>{slot.length > 1 && <small>点击查看本时段全部记录</small>}</> : <span>此时段没有检测记录</span>}<Tooltip.Arrow/></Tooltip.Content></Tooltip.Portal></Tooltip.Root>;
        })}</div>
        <footer className="evaluation-timeline-axis"><span>24 小时前</span><span>每格 30 分钟</span><span>现在</span></footer>
        <button className="evaluation-latest-result" onClick={() => onSelect(latest)}><span><i className={latest.status}/>{resultLabels[latest.status]}</span><span>查看最近一次回答 <span aria-hidden="true">↗</span></span></button>
      </section>;
    })}
    {selectedSlot && <EvaluationDialog title="时段检测记录" subtitle={`${selectedSlot[0].channelName} · ${selectedSlot[0].modelId} · ${selectedSlot.length} 次检测`} onClose={() => setSelectedSlot(null)} className="evaluation-history-dialog"><div className="evaluation-report-scroll">{selectedSlot.map((record) => <button key={record.id} className="evaluation-slot-record" onClick={() => { setSelectedSlot(null); onSelect(record); }}><i className={record.status}/><span><strong>{recordTime(record.createdAt)}</strong><small>{record.trigger === 'scheduled' ? '定时检测' : '手动检测'} · {duration(record.elapsedMs)}</small></span><span>{resultLabels[record.status]}</span></button>)}</div></EvaluationDialog>}
  </div></Tooltip.Provider>;
}
