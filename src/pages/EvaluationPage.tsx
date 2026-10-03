import { ArrowDownToLine, ArrowRight, Bird, Candy, Check, CheckCheck, ChevronRight, Clock3, Copy, FlaskConical, LoaderCircle, Play, Settings2, Square } from 'lucide-react';
import { useEffect, useState } from 'react';
import { EvaluationPlanDrawer } from '../components/evaluation/PlanDrawer';
import { EvaluationResultDetail } from '../components/evaluation/ResultDetail';
import { Select } from '../components/Select';
import { useEvaluations } from '../hooks/useEvaluations';
import { desktop } from '../lib/api';
import { evaluationApi, targetKey, type CaseResult, type EvaluationPlan, type EvaluationRun, type RunSummary } from '../lib/evaluation';
import { effortLabels } from '../lib/models';
import { errorMessage, readableTime } from '../lib/utils';
import type { Dashboard } from '../types';
import './evaluation.css';

const icons = { candy: Candy, pelican: Bird, judgment: CheckCheck };
const titles = { candy: '糖果推理', pelican: '鹈鹕绘图', judgment: '模型判题' };
const statuses = { running: '进行中', completed: '已完成', cancelled: '已取消', interrupted: '已中断' };

function HistoryRow({ run, selected, onSelect }: { run: RunSummary; selected: boolean; onSelect: () => void }) {
  return <button className={`evaluation-history-row ${selected ? 'selected' : ''}`} aria-pressed={selected} onClick={onSelect}><span className="evaluation-history-dot"/><span><strong>{readableTime(run.startedAt)}</strong><small>{run.targetCount} 个模型 · {run.completedCases}/{run.totalCases} 项 · {run.trigger === 'scheduled' ? '定时' : '手动'}</small></span><span className="evaluation-history-state">{statuses[run.status]}</span></button>;
}

function ResultRows({ run, comparison, onDetail }: { run: EvaluationRun; comparison: EvaluationRun | null; onDetail: (result: CaseResult) => void }) {
  const groups = new Map<string, CaseResult[]>();
  for (const result of run.results) { const key = targetKey(result); groups.set(key, [...(groups.get(key) ?? []), result]); }
  return <div className="evaluation-result-groups">{[...groups].map(([key, results]) => <section className="evaluation-result-group" key={key} aria-label={`${results[0].channelName} ${results[0].modelId} 评测结果`}>
    <div className="evaluation-model-heading"><div><span>{results[0].channelName}</span><h3>{results[0].modelAlias || results[0].modelId}</h3>{results[0].modelAlias && <small>{results[0].modelId}</small>}</div><span className="evaluation-effort">{effortLabels[results[0].reasoningEffort ?? ''] ?? 'API 默认'}</span></div>
    {results.map((result) => {
      const Icon = icons[result.caseId];
      const before = comparison?.caseVersion === run.caseVersion ? comparison.results.find((item) => targetKey(item) === key && item.caseId === result.caseId && item.reasoningEffort === result.reasoningEffort) : null;
      const change = result.score != null && before?.score != null ? result.score - before.score : null;
      return <button className="evaluation-result-row" key={result.caseId} onClick={() => onDetail(result)} aria-label={`查看 ${result.modelId} ${titles[result.caseId]} 结果`}>
        <span className={`evaluation-case-icon ${result.status}`}><Icon size={19}/></span><span className="evaluation-case-name"><strong>{titles[result.caseId]}</strong><small>{result.error ? '请求失败' : result.caseId === 'pelican' ? 'SVG 结构检查' : `${result.checks.filter((item) => item.passed).length}/${result.checks.length} 项核对通过`}</small></span>
        <span className="evaluation-row-score"><strong>{result.score ?? '—'}<small>{result.score != null ? '/100' : ''}</small></strong>{change != null && <small className={change < 0 ? 'negative' : 'positive'}>{change > 0 ? '+' : ''}{change} 较对比记录</small>}</span><span className="evaluation-latency">{(result.elapsedMs / 1000).toFixed(1)}s</span><ChevronRight size={15}/>
      </button>;
    })}
  </section>)}</div>;
}

export function EvaluationPage({ workspace }: { workspace: Dashboard }) {
  const evaluation = useEvaluations();
  const { data } = evaluation;
  const [editing, setEditing] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<EvaluationRun | null>(null);
  const [loading, setLoading] = useState(false);
  const [comparisonId, setComparisonId] = useState('none');
  const [comparison, setComparison] = useState<EvaluationRun | null>(null);
  const [detail, setDetail] = useState<CaseResult | null>(null);
  const [readError, setReadError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exported, setExported] = useState<{ runId: string; path: string } | null>(null);
  const [copiedPath, setCopiedPath] = useState(false);
  const [copyError, setCopyError] = useState<string | null>(null);
  const activeId = data?.active?.id;
  const shownId = selectedId ?? activeId ?? data?.history[0]?.id ?? null;
  const run = shownId === activeId ? data?.active ?? null : loaded?.id === shownId ? loaded : null;
  useEffect(() => {
    if (!shownId || shownId === activeId) return;
    let disposed = false;
    setLoading(true); setReadError(null);
    void evaluationApi.run(shownId).then((value) => { if (!disposed) setLoaded(value); }).catch((error) => { if (!disposed) setReadError(errorMessage(error)); }).finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [shownId, activeId]);
  useEffect(() => {
    setComparison(null);
    if (comparisonId === 'none' || comparisonId === shownId) return;
    let disposed = false;
    void evaluationApi.run(comparisonId).then((value) => { if (!disposed) setComparison(value); }).catch((error) => { if (!disposed) setReadError(errorMessage(error)); });
    return () => { disposed = true; };
  }, [comparisonId, shownId]);
  function initialPlan(): EvaluationPlan {
    const saved = structuredClone(data!.plan);
    if (!saved.targets.length) {
      const entry = workspace.catalog.find((item) => item.enabled && item.routeId === workspace.defaultRouteId) ?? workspace.catalog.find((item) => item.enabled);
      if (entry) saved.targets = [{ profileId: entry.profileId, modelId: entry.modelId, reasoningEffort: null }];
    }
    return saved;
  }
  async function exportRun() {
    if (!run || exporting) return;
    setExporting(true); setReadError(null);
    try {
      const file = await evaluationApi.export(run.id);
      if (desktop) {
        setExported({ runId: run.id, path: file.path }); setCopiedPath(false); setCopyError(null);
      } else {
        const href = URL.createObjectURL(new Blob([file.content], { type: 'application/json;charset=utf-8' }));
        const link = document.createElement('a'); link.href = href; link.download = file.fileName; link.click();
        setTimeout(() => URL.revokeObjectURL(href), 1000);
      }
    }
    catch (error) { setReadError(errorMessage(error)); }
    finally { setExporting(false); }
  }
  async function copyExportPath() {
    if (!exported) return;
    try { await navigator.clipboard.writeText(exported.path); setCopiedPath(true); setCopyError(null); }
    catch { setCopiedPath(false); setCopyError('无法自动复制，请选中路径后手动复制。'); }
  }
  return <div className="view-enter evaluation-page">
    <div className="page-heading"><div><h1>模型评测</h1><p className="evaluation-subtitle">推理、绘图与判题，保留每次回答的证据。</p></div><div className="evaluation-heading-actions"><button className="button button-quiet" disabled={!data || evaluation.pending} onClick={() => setEditing(true)}><Settings2 size={16}/>评测计划</button><button className="button button-primary" disabled={!data || !!data.active || evaluation.pending} onClick={() => setEditing(true)}><Play size={15}/>开始评测</button></div></div>
    {(evaluation.error || data?.error || readError) && <p className="inline-error" role="alert">{evaluation.error || data?.error || readError}</p>}
    {!data ? <div className="evaluation-loading"><LoaderCircle className="spin" size={25}/><span>正在读取评测记录</span>{evaluation.error && <button className="button button-quiet" onClick={() => void evaluation.refresh()}>重试</button>}</div> : <>
      <div className="evaluation-schedule-bar"><Clock3 size={16}/><span>{data.plan.scheduleEnabled ? `每 ${data.plan.intervalHours} 小时自动评测` : '定时评测未开启'}</span>{data.nextRunAt && <small>{desktop ? '下次' : '演示下次'} {readableTime(data.nextRunAt)}</small>}<button onClick={() => data.plan.scheduleEnabled ? void evaluation.save({ ...data.plan, scheduleEnabled: false }) : setEditing(true)} disabled={evaluation.pending}>{data.plan.scheduleEnabled ? '暂停定时' : '设置定时'}<ArrowRight size={13}/></button></div>
      {data.active && <section className="evaluation-progress" aria-label="评测进度"><div><LoaderCircle className="spin" size={19}/><p role="status">正在评测 <strong>{data.active.completedCases}/{data.active.totalCases}</strong></p><button className="button button-quiet" disabled={evaluation.pending} onClick={() => void evaluation.cancel(data.active!.id)}><Square size={13}/>停止</button></div><progress aria-label="评测完成进度" max={data.active.totalCases} value={data.active.completedCases}/><small>按顺序执行，结果随完成更新。停止后不再发送后续请求。</small></section>}
      <div className="evaluation-layout"><section className="evaluation-main-results" aria-label="评测结果">
        {run ? <><div className="evaluation-results-heading"><div><h2>{statuses[run.status]}</h2><small>{readableTime(run.startedAt)} · {run.targetCount} 个模型 · 题库 {run.caseVersion}</small></div><button className="icon-button" aria-label="导出评测报告" disabled={exporting} onClick={() => void exportRun()}><ArrowDownToLine size={18}/></button></div>
          {exported?.runId === run.id && <div className="evaluation-export"><p role="status"><Check size={14}/>{copiedPath ? '报告已保存，路径已复制。' : '报告已保存'}</p><div><input aria-label="报告保存路径" value={exported.path} readOnly onFocus={(event) => event.currentTarget.select()}/><button className="button button-quiet" onClick={() => void copyExportPath()}>{copiedPath ? <Check size={14}/> : <Copy size={14}/>}{copiedPath ? '已复制' : '复制路径'}</button></div>{copyError && <small role="alert">{copyError}</small>}</div>}
          {!!data.history.filter((item) => item.id !== shownId).length && <div className="evaluation-comparison"><Select ariaLabel="对比历史评测" value={comparisonId === shownId ? 'none' : comparisonId} options={[{ value: 'none', label: '不对比历史' }, ...data.history.filter((item) => item.id !== shownId).map((item) => ({ value: item.id, label: `${readableTime(item.startedAt)} · ${item.targetCount} 个模型` }))]} onChange={setComparisonId}/>{comparison && <small>{comparison.caseVersion === run.caseVersion ? '对比相同模型、档位与测试项目' : '题库版本不同，保留结果、不比较分数'}</small>}</div>}
          {run.error && <p className="inline-error">{run.error}</p>}
          {run.results.length ? <ResultRows run={run} comparison={comparison} onDetail={setDetail}/> : <div className="evaluation-waiting"><FlaskConical size={26}/><span>{run.status === 'running' ? '等待第一项结果' : '这轮没有完成的结果'}</span></div>}
          <p className="evaluation-footnote">分数反映这组题目的表现，不用于证明模型身份或衡量完整能力。{!desktop && ' 当前全部为演示结果。'}</p>
        </> : loading ? <div className="evaluation-waiting"><LoaderCircle className="spin" size={25}/><span>正在读取报告</span></div> : <div className="evaluation-empty"><span className="evaluation-empty-mark"><FlaskConical size={34}/></span><h2>从一轮评测开始</h2><p>选择一个或多个渠道模型，检查答案、查看绘图，追踪表现变化。</p><div className="evaluation-empty-tests">{data.cases.map((test) => { const Icon = icons[test.id]; return <span key={test.id}><Icon size={19}/>{test.title}</span>; })}</div><button className="button button-soft" onClick={() => setEditing(true)}>配置首轮评测<ArrowRight size={15}/></button><small>{desktop ? '仅在手动开始或启用定时后调用模型。' : '演示模式，不调用真实模型。'}</small></div>}
      </section><aside className="evaluation-history" aria-label="评测历史"><div className="evaluation-history-heading"><h2>最近记录</h2><span>{data.history.length}</span></div>{data.active && <HistoryRow run={data.active} selected={shownId === data.active.id} onSelect={() => setSelectedId(data.active!.id)}/>} {data.history.map((item) => <HistoryRow key={item.id} run={item} selected={shownId === item.id} onSelect={() => { setSelectedId(item.id); setDetail(null); }}/>) }{!data.history.length && !data.active && <p>完成评测后，记录会保留在这里。</p>}</aside></div>
      {editing && <EvaluationPlanDrawer initial={initialPlan()} workspace={workspace} cases={data.cases} pending={evaluation.pending} requestError={evaluation.error} running={!!data.active} onClose={() => setEditing(false)} onSave={evaluation.save} onStart={async (plan) => { const started = await evaluation.start(plan); if (started) { setSelectedId(null); setComparisonId('none'); } return started; }}/>}
      {detail && <EvaluationResultDetail key={`${targetKey(detail)}-${detail.caseId}`} result={detail} title={titles[detail.caseId]} onClose={() => setDetail(null)}/>}
    </>}
  </div>;
}
