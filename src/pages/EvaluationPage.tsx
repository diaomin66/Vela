import { Clock3, FlaskConical, LoaderCircle, Square } from 'lucide-react';
import { useCallback, useState } from 'react';
import { EvaluationDeleteDialog } from '../components/evaluation/EvaluationDeleteDialog';
import { EvaluationHistory } from '../components/evaluation/EvaluationHistory';
import { EvaluationReport } from '../components/evaluation/EvaluationReport';
import { EvaluationResults } from '../components/evaluation/EvaluationResults';
import { EvaluationHeading, EvaluationSchedule, type EvaluationMode } from '../components/evaluation/EvaluationToolbar';
import { EvaluationPlanDrawer } from '../components/evaluation/PlanDrawer';
import type { EvaluationRecord } from '../components/evaluation/presentation';
import { useEvaluations } from '../hooks/useEvaluations';
import { desktop } from '../lib/api';
import type { EvaluationCaseId, EvaluationPlan } from '../lib/evaluation';
import type { Dashboard } from '../types';
import './evaluation.css';

export function EvaluationPage({ workspace }: { workspace: Dashboard }) {
  const evaluation = useEvaluations();
  const { data, activity } = evaluation;
  const [mode, setMode] = useState<EvaluationMode>('manual');
  const [editing, setEditing] = useState(false);
  const [manualDraft, setManualDraft] = useState<EvaluationPlan | null>(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [cases, setCases] = useState<Record<EvaluationMode, EvaluationCaseId>>({ manual: 'pelican', scheduled: 'candy' });
  const [report, setReport] = useState<{ runId: string; record: EvaluationRecord | null } | null>(null);
  const [deleting, setDeleting] = useState<string[] | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const records = (activity?.records ?? []).filter((record) => record.trigger === mode);
  const history = data?.history.filter((run) => run.trigger === mode) ?? [];
  const active = data?.active?.trigger === mode ? data.active : null;
  function initialPlan(): EvaluationPlan {
    const saved = structuredClone(mode === 'manual' && manualDraft ? manualDraft : data!.plan);
    if (!saved.targets.length) {
      const entry = workspace.catalog.find((item) => item.enabled && item.routeId === workspace.defaultRouteId) ?? workspace.catalog.find((item) => item.enabled);
      if (entry) saved.targets = [{ profileId: entry.profileId, modelId: entry.modelId, reasoningEffort: null }];
    }
    if (mode === 'manual') saved.scheduleEnabled = false;
    else if (!data!.plan.targets.length) saved.scheduleEnabled = true;
    return saved;
  }
  const selectRecord = useCallback((record: EvaluationRecord) => setReport({ runId: record.runId, record }), []);
  function requestDelete(ids: string[]) { setDeleteError(null); setDeleting(ids); }
  async function remove() {
    if (!deleting) return;
    const ids = deleting;
    if (await evaluation.remove(ids)) {
      if (report && ids.includes(report.runId)) setReport(null);
      setDeleting(null);
    } else setDeleteError('删除未完成，请重试；记录状态以刷新结果为准。');
  }
  return <div className="view-enter evaluation-page">
    <nav className="evaluation-subnav" aria-label="评测子导航"><button aria-current={mode === 'manual' ? 'page' : undefined} onClick={() => setMode('manual')}><FlaskConical size={16}/>单次检测</button><button aria-current={mode === 'scheduled' ? 'page' : undefined} onClick={() => setMode('scheduled')}><Clock3 size={16}/>定时评测</button></nav>
    <EvaluationHeading mode={mode} ready={!!data} pending={evaluation.pending} running={!!data?.active} onPlan={() => setEditing(true)} onHistory={() => setHistoryOpen(true)}/>
    {(evaluation.error || data?.error) && <p className="inline-error" role="alert">{evaluation.error || data?.error}</p>}
    {!data ? <div className="evaluation-loading"><LoaderCircle className="spin" size={25}/><span>正在读取评测记录</span>{evaluation.error && <button className="button button-quiet" onClick={() => void evaluation.refresh()}>重试</button>}</div> : <>
      {mode === 'scheduled' && <EvaluationSchedule data={data} pending={evaluation.pending} onEdit={() => setEditing(true)} onToggle={() => void evaluation.save({ ...data.plan, scheduleEnabled: !data.plan.scheduleEnabled })}/>}
      {data.active && <section className="evaluation-progress" aria-label="评测进度"><div><LoaderCircle className="spin" size={19}/><p role="status">{data.active.trigger === 'manual' ? '单次检测' : '定时评测'}进行中 <strong>{data.active.completedCases} / {data.active.totalCases}</strong></p><button className="button button-quiet" disabled={evaluation.pending} onClick={() => void evaluation.cancel(data.active!.id)}><Square size={13}/>停止</button></div><progress aria-label="评测完成进度" max={data.active.totalCases} value={data.active.completedCases}/></section>}
      <EvaluationResults key={mode} records={records} mode={mode} caseId={cases[mode]} onCase={(id) => setCases((previous) => ({ ...previous, [mode]: id }))} loaded={!!activity} refreshing={evaluation.refreshing} onRefresh={() => void evaluation.refresh()} onSelect={selectRecord} onCreate={() => setEditing(true)}/>
      {!desktop && <p className="evaluation-demo-note">演示数据 · 不调用真实模型</p>}
      {editing && <EvaluationPlanDrawer mode={mode} initial={initialPlan()} workspace={workspace} cases={data.cases} pending={evaluation.pending} requestError={evaluation.error} running={!!data.active} onClose={() => setEditing(false)} onSave={evaluation.save} onStart={async (plan) => { const started = await evaluation.start(plan); if (started) { setManualDraft(plan); if (!plan.cases.includes(cases.manual)) setCases((previous) => ({ ...previous, manual: plan.cases[0] })); } return started; }}/>}
      {historyOpen && <EvaluationHistory scope={mode} history={history} active={active} pending={evaluation.pending} onDelete={requestDelete} onClose={() => setHistoryOpen(false)} onSelect={(runId) => { setHistoryOpen(false); setReport({ runId, record: null }); }}/>}
      {report && <EvaluationReport key={`${report.runId}:${report.record?.id ?? ''}`} runId={report.runId} selected={report.record} history={history} pending={evaluation.pending} onDelete={requestDelete} onClose={() => setReport(null)}/>}
      {deleting && <EvaluationDeleteDialog count={deleting.length} pending={evaluation.pending} error={deleteError ? evaluation.error ?? deleteError : null} onCancel={() => setDeleting(null)} onConfirm={() => void remove()}/>}
    </>}
  </div>;
}
