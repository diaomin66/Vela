import { Clock3, Plus, X } from 'lucide-react';
import { useState } from 'react';
import { desktop } from '../../lib/api';
import { requestBudget, targetKey, validateEvaluationPlan, type EvaluationCase, type EvaluationPlan, type EvaluationTarget } from '../../lib/evaluation';
import { effortLabels } from '../../lib/models';
import type { CatalogEntry, Dashboard } from '../../types';
import { Select } from '../Select';
import { EvaluationDialog } from './EvaluationDialog';
import { intervalLabel } from './presentation';

const REQUEST_TIMEOUT_PRESETS = [120, 300, 600, 1800];

function EffortSelect({ entry, value, onChange, label }: { entry?: CatalogEntry; value: string | null; onChange: (value: string | null) => void; label: string }) {
  const efforts = entry?.apiReasoningEfforts ?? [];
  return <Select ariaLabel={label} value={value ?? 'auto'} options={[{ value: 'auto', label: 'API 默认' }, ...efforts.map((effort) => ({ value: effort, label: effortLabels[effort] ?? effort }))]} onChange={(next) => onChange(next === 'auto' ? null : next)}/>;
}

export function EvaluationPlanDrawer({ initial, workspace, cases, pending, requestError, onClose, onSave, onStart, running, mode }: {
  initial: EvaluationPlan; workspace: Dashboard; cases: EvaluationCase[]; pending: boolean; requestError: string | null; running: boolean;
  onClose: () => void; onSave: (plan: EvaluationPlan) => Promise<boolean>; onStart: (plan: EvaluationPlan) => Promise<boolean>;
  mode: 'manual' | 'scheduled';
}) {
  const [plan, setPlan] = useState(() => ({ ...structuredClone(initial), scheduleEnabled: mode === 'manual' ? false : initial.scheduleEnabled }));
  const [customTimeout, setCustomTimeout] = useState(!REQUEST_TIMEOUT_PRESETS.includes(initial.requestTimeoutSeconds));
  const [error, setError] = useState<string | null>(null);
  const entries = workspace.catalog.filter((entry) => entry.enabled);
  const entriesByKey = new Map(entries.map((entry) => [targetKey(entry), entry]));
  const options = entries.map((entry) => ({ value: targetKey(entry), label: `${entry.channelName} · ${entry.modelId}`, description: entry.displayName }));
  const available = options.filter((option) => !plan.targets.some((target) => targetKey(target) === option.value));
  const [nextTarget, setNextTarget] = useState('');
  const patch = (change: Partial<EvaluationPlan>) => { setPlan((previous) => ({ ...previous, ...change })); setError(null); };
  const targetFor = (key: string): EvaluationTarget | null => { const entry = entriesByKey.get(key); return entry ? { profileId: entry.profileId, modelId: entry.modelId, reasoningEffort: null } : null; };
  const selfJudging = plan.judge && plan.targets.some((target) => targetKey(target) === targetKey(plan.judge!));
  const intervalMinutes = plan.intervalMinutes ?? plan.intervalHours * 60;
  async function submit(start: boolean) {
    const problem = validateEvaluationPlan(plan) ?? (plan.targets.some((target) => !entriesByKey.has(targetKey(target))) ? '有模型已停用或删除，请移除后重试。' : null);
    if (problem) { setError(problem); return; }
    if (await (start ? onStart(plan) : onSave(plan))) onClose();
  }
  return <EvaluationDialog title={mode === 'manual' ? '新建检测' : '定时计划'} subtitle={mode === 'manual' ? '选择模型和题目，执行一轮检测' : '保存后按计划在后台执行'} onClose={onClose} locked={pending} className="evaluation-plan-dialog">
    <form className="editor-form evaluation-plan" noValidate onSubmit={(event) => { event.preventDefault(); void submit(mode === 'manual'); }}>
      <div className="editor-scroll">
        <section className="evaluation-plan-section"><div className="evaluation-section-label"><h3>被测模型</h3><span>{plan.targets.length} / 6</span></div>
          <div className="evaluation-targets">{plan.targets.map((target, index) => {
            const entry = entriesByKey.get(targetKey(target));
            return <div className="evaluation-target" key={targetKey(target)}><div><strong>{entry?.channelName ?? '模型不可用'}</strong><span>{target.modelId}</span></div><button type="button" className="icon-button" aria-label={`移除被测模型 ${target.modelId}`} disabled={pending} onClick={() => patch({ targets: plan.targets.filter((_, i) => i !== index) })}><X size={15}/></button><EffortSelect entry={entry} value={target.reasoningEffort} label={`${target.modelId} 的评测推理强度`} onChange={(reasoningEffort) => patch({ targets: plan.targets.map((item, i) => i === index ? { ...item, reasoningEffort } : item) })}/></div>;
          })}</div>
          <div className="evaluation-add-target"><Select ariaLabel="添加被测模型" value={nextTarget} placeholder={available.length ? '选择渠道与模型' : entries.length ? '已选择全部可用模型' : '先在渠道中启用模型'} options={available} searchable disabled={pending || plan.targets.length >= 6 || !available.length} onChange={setNextTarget}/><button type="button" className="button button-soft" aria-label="添加所选模型" disabled={pending || !nextTarget || plan.targets.length >= 6} onClick={() => { const target = targetFor(nextTarget); if (target && !plan.targets.some((item) => targetKey(item) === nextTarget)) patch({ targets: [...plan.targets, target] }); setNextTarget(''); }}><Plus size={17}/></button></div>
          <p className="evaluation-caption">直接调用 API，使用普通推理档位；Ultra 在原生任务中使用。</p>
        </section>
        <section className="evaluation-plan-section"><div className="evaluation-section-label"><h3>测试项目</h3></div><div className="evaluation-case-choices">{cases.map((test) => <label className={plan.cases.includes(test.id) ? 'selected' : ''} key={test.id}><input type="checkbox" checked={plan.cases.includes(test.id)} disabled={pending} onChange={(event) => patch({ cases: event.target.checked ? [...plan.cases, test.id] : plan.cases.filter((id) => id !== test.id) })}/><span><strong>{test.title}</strong><small>{test.description}</small></span></label>)}</div></section>
        {plan.cases.some((id) => id !== 'pelican') && <section className="evaluation-plan-section"><div className="evaluation-section-label"><h3>模型复评</h3><span>可选</span></div><Select ariaLabel="评审模型" value={plan.judge ? targetKey(plan.judge) : 'off'} searchable options={[{ value: 'off', label: '不使用模型复评', description: '保留答案核对与原始回答' }, ...options]} onChange={(value) => patch({ judge: value === 'off' ? null : targetFor(value) })} disabled={pending}/>
          {plan.judge && <div className="evaluation-judge-effort"><EffortSelect entry={entriesByKey.get(targetKey(plan.judge))} value={plan.judge.reasoningEffort} label="评审模型推理强度" onChange={(reasoningEffort) => patch({ judge: { ...plan.judge!, reasoningEffort } })}/></div>}
          <p className="evaluation-caption">{selfJudging ? '当前包含同模型自评，结果会单独标注。' : '仅复评糖果与判题答案；鹈鹕作品直接展示。'}</p>
        </section>}
        <section className="evaluation-plan-section"><div className="evaluation-section-label"><label htmlFor="evaluation-request-timeout">单次请求超时</label><span>默认 5 分钟</span></div><Select id="evaluation-request-timeout" ariaLabel="单次请求超时" value={customTimeout ? 'custom' : String(plan.requestTimeoutSeconds)} options={[...REQUEST_TIMEOUT_PRESETS.map((seconds) => ({ value: String(seconds), label: `${seconds / 60} 分钟` })), { value: 'custom', label: '自定义' }]} disabled={pending} onChange={(value) => { setCustomTimeout(value === 'custom'); if (value !== 'custom') patch({ requestTimeoutSeconds: Number(value) }); }}/>
          {customTimeout && <div className="evaluation-timeout-custom"><label htmlFor="evaluation-timeout-seconds">超时秒数</label><input id="evaluation-timeout-seconds" type="number" min={30} max={3600} step={1} required value={Number.isFinite(plan.requestTimeoutSeconds) ? plan.requestTimeoutSeconds : ''} aria-describedby="evaluation-timeout-hint" aria-invalid={error?.startsWith('请求超时') || undefined} disabled={pending} onChange={(event) => patch({ requestTimeoutSeconds: event.target.valueAsNumber })}/></div>}
          <p id="evaluation-timeout-hint" className="evaluation-caption">每道题与模型复评分别计时，支持 30–3600 秒。</p>
        </section>
        {mode === 'scheduled' && <section className="evaluation-plan-section"><label className="evaluation-schedule-toggle" htmlFor="evaluation-schedule"><span><Clock3 size={17}/><strong>启用定时</strong></span><input id="evaluation-schedule" type="checkbox" checked={plan.scheduleEnabled} disabled={pending} onChange={(event) => patch({ scheduleEnabled: event.target.checked })}/></label>
          <div className="evaluation-interval"><Select ariaLabel="评测间隔" value={String(intervalMinutes)} options={[...new Set([10, 30, 60, 180, 360, 1440, 10080, intervalMinutes])].sort((a, b) => a - b).map((minutes) => ({ value: String(minutes), label: intervalLabel(minutes) }))} onChange={(value) => patch({ intervalMinutes: Number(value), intervalHours: Math.max(1, Math.ceil(Number(value) / 60)) })} disabled={pending}/><p className="evaluation-caption">AhaX 在后台运行时执行，关闭窗口不会停止计划。</p></div>
        </section>}
        <p className="evaluation-cost">{desktop ? `每轮最多 ${requestBudget(plan)} 次模型请求，按渠道计费。` : `演示每轮 ${requestBudget(plan)} 次请求，不发送 API 请求或执行真实定时任务。`}</p>
        {(error || requestError) && <p className="inline-error" role="alert">{error || requestError}</p>}
      </div>
      <footer className="evaluation-drawer-footer"><button type="button" className="button button-quiet" disabled={pending} onClick={onClose}>取消</button><button type="submit" className="button button-primary" disabled={pending || mode === 'manual' && (running || !entries.length)}>{pending ? '正在处理…' : mode === 'manual' ? '开始检测' : '保存计划'}</button></footer>
    </form>
  </EvaluationDialog>;
}
