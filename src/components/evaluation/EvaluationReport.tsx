import { History, LoaderCircle, Trash2 } from 'lucide-react';
import { useState } from 'react';
import { useEvaluationRun } from '../../hooks/useEvaluations';
import type { CaseResult, RunSummary } from '../../lib/evaluation';
import { readableTime } from '../../lib/utils';
import { Select } from '../Select';
import { EvaluationDialog } from './EvaluationDialog';
import { EvaluationResultContent } from './ResultDetail';
import { ReportExport } from './ReportExport';
import { caseTitles, recordKey, runLabels, type EvaluationRecord } from './presentation';

const resultId = (result: CaseResult) => `${recordKey(result)}:${result.caseId}`;

export function EvaluationReport({ runId, selected, history, onClose, onDelete, pending }: { runId: string; selected: EvaluationRecord | null; history: RunSummary[]; onClose: () => void; onDelete: (ids: string[]) => void; pending: boolean }) {
  const query = useEvaluationRun(runId);
  const [selection, setSelection] = useState(selected ? `${recordKey(selected)}:${selected.caseId}` : '');
  const [comparisonId, setComparisonId] = useState('none');
  const comparison = useEvaluationRun(comparisonId === 'none' ? null : comparisonId);
  const run = query.data;
  const result = run?.results.find((item) => resultId(item) === selection) ?? run?.results[0];
  const title = selected ? caseTitles[selected.caseId] : '评测报告';
  const earlier = result && comparison.data?.caseVersion === run?.caseVersion ? comparison.data?.results.find((item) => resultId(item) === resultId(result)) : null;
  return <EvaluationDialog title={title} subtitle={run ? `${readableTime(run.startedAt)} · ${runLabels[run.status]}` : '正在读取本次记录'} onClose={onClose} className="evaluation-report-dialog">
    <div className="evaluation-report-scroll">
      {query.error && <p className="inline-error" role="alert">报告读取失败，请重试。<button className="button button-quiet" onClick={() => void query.refetch()}>重试</button></p>}
      {!run && query.isPending && <div className="evaluation-loading"><LoaderCircle className="spin" size={25}/><span>正在读取报告</span></div>}
      {run && <><div className="evaluation-report-top"><span>{run.targetCount} 个模型 · {run.completedCases} / {run.totalCases} 项完成</span><div className="evaluation-report-actions"><button className="icon-button evaluation-delete-record" aria-label="删除本轮评测" disabled={pending || run.status === 'running'} onClick={() => onDelete([run.id])}><Trash2 size={16}/></button><ReportExport key={run.id} runId={run.id}/></div></div>
        {run.error && <p className="inline-error" role="alert">{run.error}</p>}
        {result ? <>
          {run.results.length > 1 ? <div className="evaluation-report-picker"><Select ariaLabel="选择评测结果" value={resultId(result)} options={run.results.map((item) => ({ value: resultId(item), label: `${item.channelName} · ${item.modelId}`, description: caseTitles[item.caseId] }))} onChange={setSelection}/></div> : <div className="evaluation-report-model"><span>{result.channelName}</span><h3>{result.modelId}</h3></div>}
          <EvaluationResultContent key={resultId(result)} result={result}/>
          {result.caseId !== 'pelican' && history.some((item) => item.id !== run.id) && <div className="evaluation-comparison"><Select ariaLabel="对比历史评测" value={comparisonId} options={[{ value: 'none', label: '不对比历史' }, ...history.filter((item) => item.id !== run.id).map((item) => ({ value: item.id, label: `${readableTime(item.startedAt)} · ${item.targetCount} 个模型` }))]} onChange={setComparisonId}/>{comparison.data && <p>{comparison.data.caseVersion !== run.caseVersion ? '题库版本不同，不比较分数。' : earlier?.score != null && result.score != null ? `${result.score - earlier.score > 0 ? '+' : ''}${result.score - earlier.score} 较对比记录 · 相同模型、档位与测试项目` : '该轮没有可比较的相同模型与档位。'}</p>}</div>}
        </> : <div className="evaluation-report-empty"><History size={26}/><p>这轮没有完成的结果</p></div>}
      </>}
    </div>
  </EvaluationDialog>;
}
