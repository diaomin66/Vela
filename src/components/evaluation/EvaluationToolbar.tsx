import * as Tabs from '@radix-ui/react-tabs';
import { Bird, Candy, CheckCheck, Clock3, History, Play, RefreshCw, Settings2 } from 'lucide-react';
import type { EvaluationCaseId, EvaluationDashboard } from '../../lib/evaluation';
import { desktop } from '../../lib/api';
import { readableTime } from '../../lib/utils';
import { PageHeader } from '../ui/Workspace';
import { caseTitles, intervalLabel, type EvaluationRecord } from './presentation';

const icons = { pelican: Bird, candy: Candy, judgment: CheckCheck };
const cases: EvaluationCaseId[] = ['pelican', 'candy', 'judgment'];
export type EvaluationMode = 'manual' | 'scheduled';

export function EvaluationHeading({ mode, ready, pending, running, onPlan, onHistory }: { mode: EvaluationMode; ready: boolean; pending: boolean; running: boolean; onPlan: () => void; onHistory: () => void }) {
  return <PageHeader title={mode === 'manual' ? '单次检测' : '定时评测'} description={mode === 'manual' ? '测试模型表现，比较不同渠道的回答与作品' : '按计划检测模型，持续观察回答质量与稳定性'} actions={<><button className="button button-quiet" onClick={onHistory} disabled={!ready}><History size={16}/>记录</button><button className="button button-primary" disabled={!ready || pending || mode === 'manual' && running} onClick={onPlan}>{mode === 'manual' ? <Play size={15}/> : <Settings2 size={16}/>} {mode === 'manual' ? '开始检测' : '编辑计划'}</button></>}/>;
}

export function EvaluationToolbar({ records, channel, onChannel, onRefresh, refreshing }: { records: EvaluationRecord[]; channel: string; onChannel: (channel: string) => void; onRefresh: () => void; refreshing: boolean }) {
  const groups = new Map<string, { name: string; count: number }>();
  for (const record of records) { const value = groups.get(record.profileId); if (value) value.count++; else groups.set(record.profileId, { name: record.channelName, count: 1 }); }
  return <div className="evaluation-toolbar">
    <div className="evaluation-toolbar-top"><Tabs.List className="evaluation-case-tabs segmented-control" aria-label="评测项目">{cases.map((id) => { const Icon = icons[id]; return <Tabs.Trigger value={id} key={id}><Icon size={17}/>{caseTitles[id]}</Tabs.Trigger>; })}</Tabs.List><button className="icon-button" aria-label="刷新评测记录" disabled={refreshing} onClick={onRefresh}><RefreshCw size={17} className={refreshing ? 'spin' : undefined}/></button></div>
    {!!records.length && <div className="evaluation-channel-filters" aria-label="筛选渠道"><button className={channel === 'all' ? 'selected' : ''} aria-pressed={channel === 'all'} onClick={() => onChannel('all')}>全部渠道<span>{records.length}</span></button>{[...groups].map(([id, group]) => <button key={id} className={channel === id ? 'selected' : ''} aria-pressed={channel === id} onClick={() => onChannel(id)}>{group.name}<span>{group.count}</span></button>)}</div>}
  </div>;
}

export function EvaluationSchedule({ data, pending, onEdit, onToggle }: { data: EvaluationDashboard; pending: boolean; onEdit: () => void; onToggle: () => void }) {
  const configured = data.plan.targets.length > 0;
  const enabled = data.plan.scheduleEnabled;
  const interval = data.plan.intervalMinutes ?? data.plan.intervalHours * 60;
  return <section className="evaluation-schedule-panel" aria-label="定时计划概况"><div className="evaluation-schedule-title"><span className="evaluation-channel-mark"><Clock3 size={20}/></span><div><h2>{enabled ? '计划运行中' : configured ? '计划已暂停' : '设置你的第一份计划'}</h2><p>{configured ? `${data.plan.targets.length} 个模型 · ${data.plan.cases.map((id) => caseTitles[id]).join('、')}` : '选择模型、题目与间隔，后续检测自动进行。'}</p></div></div><div className="evaluation-schedule-facts"><span><small>执行间隔</small><strong>{intervalLabel(interval)}</strong></span><span><small>下次检测</small><strong>{enabled && data.nextRunAt ? `${desktop ? '' : '演示 · '}${readableTime(data.nextRunAt)}` : '—'}</strong></span><span><small>请求超时</small><strong>{data.plan.requestTimeoutSeconds} 秒</strong></span></div><button className={`button ${configured ? 'button-quiet' : 'button-primary'}`} disabled={pending} onClick={configured ? onToggle : onEdit}>{configured ? enabled ? '暂停计划' : '启用计划' : '设置计划'}</button></section>;
}
