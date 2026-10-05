import * as Tabs from '@radix-ui/react-tabs';
import { Check, CircleAlert, Code2, FileText, Image, RotateCcw, X } from 'lucide-react';
import { useState } from 'react';
import type { CaseResult } from '../../lib/evaluation';
import { ArtifactPreview } from './ArtifactPreview';
import { duration, effortName, resultLabels } from './presentation';

function ArtifactReview({ html, modelId }: { html: string; modelId: string }) {
  const [revision, setRevision] = useState(0);
  return <div className="evaluation-artifact-review"><div className="evaluation-artifact-controls"><span>动态作品</span><button onClick={() => setRevision((value) => value + 1)}><RotateCcw size={14}/>重新播放</button></div><div className="evaluation-detail-artifact"><ArtifactPreview key={revision} html={html} title={`${modelId} 鹈鹕动画完整预览`} interactive/></div></div>;
}

export function EvaluationResultContent({ result }: { result: CaseResult }) {
  const html = result.artifactHtml ?? result.safeSvg;
  return <div className="evaluation-result-detail">
    <dl className="evaluation-result-meta"><div><dt>推理强度</dt><dd>{effortName(result.reasoningEffort)}</dd></div><div><dt>请求耗时</dt><dd>{duration(result.elapsedMs)}</dd></div>{result.inputTokens != null && <div><dt>Token · 输入 / 输出</dt><dd>{result.inputTokens.toLocaleString()} / {result.outputTokens?.toLocaleString() ?? '—'}</dd></div>}</dl>
    <Tabs.Root defaultValue="result"><Tabs.List className="evaluation-detail-tabs segmented-control" aria-label="结果内容"><Tabs.Trigger value="result"><Image size={16}/>结果</Tabs.Trigger><Tabs.Trigger value="prompt"><FileText size={16}/>题目</Tabs.Trigger><Tabs.Trigger value="output"><Code2 size={16}/>原文</Tabs.Trigger></Tabs.List>
      <Tabs.Content value="prompt"><pre className="evaluation-raw">{result.prompt}</pre></Tabs.Content>
      <Tabs.Content value="output"><pre className="evaluation-raw">{result.output || '本次没有返回答案。'}</pre></Tabs.Content>
      <Tabs.Content value="result">
        {result.error && <p className="inline-error" role="alert"><CircleAlert size={16}/>{result.error}</p>}
        {result.caseId === 'pelican' ? <>{html ? <ArtifactReview html={html} modelId={result.modelId}/> : !result.error && <p className="evaluation-report-empty">这次没有可预览的 HTML 或 SVG，完整回答保留在原文中。</p>}{!result.artifactHtml && result.safeSvg && <p className="evaluation-caption">此作品来自旧版 SVG 记录。</p>}</> : <>
          <div className="evaluation-verdict"><span className={`evaluation-outcome ${result.status}`}>{result.status === 'passed' ? <Check size={17}/> : <CircleAlert size={17}/>} {resultLabels[result.status]}</span><span>{result.checks.filter((check) => check.passed).length} / {result.checks.length} 项核对通过</span></div>
          {result.checks.length > 0 && <ul className="evaluation-checks">{result.checks.map((check, index) => <li key={`${index}-${check.label}`} className={check.passed ? 'passed' : 'failed'}>{check.passed ? <Check size={16}/> : <X size={16}/>}<span>{check.label}</span></li>)}</ul>}
          {result.output && <section className="evaluation-answer-section"><h3>模型回答</h3><pre className="evaluation-answer">{result.output}</pre></section>}
          {result.judge && <section className="evaluation-judge-result"><div><h3>模型复评</h3><strong>{result.judge.score == null ? '未完成' : `${result.judge.score} / 100`}</strong></div><p>{result.judge.error || result.judge.explanation}</p><small>{result.judge.modelId}{result.judge.profileId === result.profileId && result.judge.modelId === result.modelId ? ' · 同模型自评' : ''}</small></section>}
        </>}
      </Tabs.Content>
    </Tabs.Root>
  </div>;
}
