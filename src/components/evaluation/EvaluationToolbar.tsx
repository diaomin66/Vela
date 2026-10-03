import * as Tabs from '@radix-ui/react-tabs';
import { Bird, Candy, CheckCheck, Clock3, History, Pause, Play, RefreshCw, Settings2 } from 'lucide-react';
import type { EvaluationCaseId, EvaluationDashboard } from '../../lib/evaluation';
import { desktop } from '../../lib/api';
import { readableTime } from '../../lib/utils';
import { caseTitles, intervalLabel, type EvaluationRecord } from './presentation';

const icons = { pelican: Bird, candy: Candy, judgment: CheckCheck };
const cases: EvaluationCaseId[] = ['pelican', 'candy', 'judgment'];

export function EvaluationHeading({ ready, pending, running, onPlan, onHistory }: { ready: boolean; pending: boolean; running: boolean; onPlan: () => void; onHistory: () => void }) {
  return <div className="page-heading"><div><h1>模型评测</h1><p className="evaluation-subtitle">看见创作，追踪每一次推理。</p></div><div className="evaluation-heading-actions"><button className="button evaluation-history-button" onClick={onHistory} disabled={!ready}><History size={16}/>记录</button><button className="button button-quiet" disabled={!ready || pending} onClick={onPlan}><Settings2 size={16}/>评测计划</button><button className="button button-primary" disabled={!ready || pending || running} onClick={onPlan}><Play size={15}/>开始评测</button></div></div>;
}

export function EvaluationToolbar({ records, channel, onChannel, onRefresh, refreshing }: { records: EvaluationRecord[]; channel: string; onChannel: (channel: string) => void; onRefresh: () => void; refreshing: boolean }) {
  const groups = new Map<string, { name: string; count: number }>();
  for (const record of records) { const value = groups.get(record.profileId); if (value) value.count++; else groups.set(record.profileId, { name: record.channelName, count: 1 }); }
  return <div className="evaluation-toolbar">
    <div className="evaluation-toolbar-top"><Tabs.List className="evaluation-case-tabs" aria-label="评测项目">{cases.map((id) => { const Icon = icons[id]; return <Tabs.Trigger value={id} key={id}><Icon size={17}/>{caseTitles[id]}</Tabs.Trigger>; })}</Tabs.List><button className="icon-button" aria-label="刷新评测记录" disabled={refreshing} onClick={onRefresh}><RefreshCw size={17} className={refreshing ? 'spin' : undefined}/></button></div>
    <div className="evaluation-channel-filters" aria-label="筛选渠道"><button className={channel === 'all' ? 'selected' : ''} aria-pressed={channel === 'all'} onClick={() => onChannel('all')}>全部渠道<span>{records.length}</span></button>{[...groups].map(([id, group]) => <button key={id} className={channel === id ? 'selected' : ''} aria-pressed={channel === id} onClick={() => onChannel(id)}>{group.name}<span>{group.count}</span></button>)}</div>
  </div>;
}

export function EvaluationSchedule({ data, pending, onEdit, onPause }: { data: EvaluationDashboard; pending: boolean; onEdit: () => void; onPause: () => void }) {
  const interval = data.plan.intervalMinutes ?? data.plan.intervalHours * 60;
  return <div className={`evaluation-schedule-bar ${data.plan.scheduleEnabled ? 'enabled' : ''}`}><Clock3 size={15}/><span>{data.plan.scheduleEnabled ? `${intervalLabel(interval)}自动评测` : '定时评测未开启'}</span>{data.plan.scheduleEnabled && data.nextRunAt && <small>{desktop ? '下次' : '演示下次'} {readableTime(data.nextRunAt)}</small>}<button disabled={pending} onClick={data.plan.scheduleEnabled ? onPause : onEdit}>{data.plan.scheduleEnabled ? <><Pause size={12}/>暂停定时</> : '设置定时'}</button></div>;
}
