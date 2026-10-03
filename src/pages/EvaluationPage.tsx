import * as Tabs from '@radix-ui/react-tabs';
import { ArrowRight, Bird, Candy, CheckCheck, LoaderCircle, Square } from 'lucide-react';
import { useCallback, useState } from 'react';
import { EvaluationGallery } from '../components/evaluation/EvaluationGallery';
import { EvaluationHistory, EvaluationReport } from '../components/evaluation/EvaluationReport';
import { EvaluationTimeline } from '../components/evaluation/EvaluationTimeline';
import { EvaluationHeading, EvaluationSchedule, EvaluationToolbar } from '../components/evaluation/EvaluationToolbar';
import { EvaluationPlanDrawer } from '../components/evaluation/PlanDrawer';
import { caseTitles, type EvaluationRecord } from '../components/evaluation/presentation';
import { useEvaluations } from '../hooks/useEvaluations';
import { desktop } from '../lib/api';
import type { EvaluationCaseId, EvaluationPlan } from '../lib/evaluation';
import type { Dashboard } from '../types';
import './evaluation.css';

const icons = { pelican: Bird, candy: Candy, judgment: CheckCheck };

export function EvaluationPage({ workspace }: { workspace: Dashboard }) {
  const evaluation = useEvaluations();
  const { data, activity } = evaluation;
  const [editing, setEditing] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [caseId, setCaseId] = useState<EvaluationCaseId>('pelican');
  const [channel, setChannel] = useState('all');
  const [report, setReport] = useState<{ runId: string; record: EvaluationRecord | null } | null>(null);
  const records = (activity?.records ?? []).filter((record) => record.caseId === caseId).sort((a, b) => b.createdAt.localeCompare(a.createdAt));
  const currentChannel = channel === 'all' || records.some((record) => record.profileId === channel) ? channel : 'all';
  const shown = currentChannel === 'all' ? records : records.filter((record) => record.profileId === currentChannel);
  const Icon = icons[caseId];
  function initialPlan(): EvaluationPlan {
    const saved = structuredClone(data!.plan);
    if (!saved.targets.length) {
      const entry = workspace.catalog.find((item) => item.enabled && item.routeId === workspace.defaultRouteId) ?? workspace.catalog.find((item) => item.enabled);
      if (entry) saved.targets = [{ profileId: entry.profileId, modelId: entry.modelId, reasoningEffort: null }];
    }
    return saved;
  }
  const selectRecord = useCallback((record: EvaluationRecord) => setReport({ runId: record.runId, record }), []);
  return <div className="view-enter evaluation-page">
    <EvaluationHeading ready={!!data} pending={evaluation.pending} running={!!data?.active} onPlan={() => setEditing(true)} onHistory={() => setHistoryOpen(true)}/>
    {(evaluation.error || data?.error) && <p className="inline-error" role="alert">{evaluation.error || data?.error}</p>}
    {!data ? <div className="evaluation-loading"><LoaderCircle className="spin" size={25}/><span>正在读取评测记录</span>{evaluation.error && <button className="button button-quiet" onClick={() => void evaluation.refresh()}>重试</button>}</div> : <>
      <EvaluationSchedule data={data} pending={evaluation.pending} onEdit={() => setEditing(true)} onPause={() => void evaluation.save({ ...data.plan, scheduleEnabled: false })}/>
      {data.active && <section className="evaluation-progress" aria-label="评测进度"><div><LoaderCircle className="spin" size={19}/><p role="status">正在评测 <strong>{data.active.completedCases} / {data.active.totalCases}</strong></p><button className="button button-quiet" disabled={evaluation.pending} onClick={() => void evaluation.cancel(data.active!.id)}><Square size={13}/>停止</button></div><progress aria-label="评测完成进度" max={data.active.totalCases} value={data.active.completedCases}/></section>}
      <Tabs.Root value={caseId} onValueChange={(value) => { setCaseId(value as EvaluationCaseId); setChannel('all'); }}>
        <EvaluationToolbar records={records} channel={currentChannel} onChannel={setChannel} onRefresh={() => void evaluation.refresh()} refreshing={evaluation.refreshing}/>
        <section aria-label="评测结果">
          {(['pelican', 'candy', 'judgment'] as const).map((id) => <Tabs.Content key={id} value={id} className="evaluation-tab-content">
            {!activity ? <div className="evaluation-loading"><LoaderCircle className="spin" size={24}/><span>正在读取作品与检测记录</span></div> : shown.length ? id === 'pelican' ? <EvaluationGallery records={shown} onSelect={selectRecord}/> : <EvaluationTimeline records={shown} caseId={id} onSelect={selectRecord}/> : <div className="evaluation-empty"><span className="evaluation-empty-mark"><Icon size={34}/></span><h2>{caseId === 'pelican' ? '让模型的创作自己说话' : `从一次${caseTitles[caseId]}开始`}</h2><p>{caseId === 'pelican' ? '同一道鹈鹕骑车题，留下每个渠道的动态作品。' : '答案按时间排列，模型表现的变化一目了然。'}</p><button className="button button-soft" onClick={() => setEditing(true)}>配置首轮评测<ArrowRight size={15}/></button></div>}
          </Tabs.Content>)}
        </section>
      </Tabs.Root>
      {!desktop && <p className="evaluation-demo-note">演示环境 · 展示示例结果，不调用真实模型。</p>}
      {editing && <EvaluationPlanDrawer initial={initialPlan()} workspace={workspace} cases={data.cases} pending={evaluation.pending} requestError={evaluation.error} running={!!data.active} onClose={() => setEditing(false)} onSave={evaluation.save} onStart={async (plan) => { const started = await evaluation.start(plan); if (started && !plan.cases.includes(caseId)) setCaseId(plan.cases[0]); return started; }}/>}
      {historyOpen && <EvaluationHistory history={data.history} active={data.active} onClose={() => setHistoryOpen(false)} onSelect={(runId) => { setHistoryOpen(false); setReport({ runId, record: null }); }}/>}
      {report && <EvaluationReport key={`${report.runId}:${report.record?.id ?? ''}`} runId={report.runId} selected={report.record} history={data.history} onClose={() => setReport(null)}/>}
    </>}
  </div>;
}
