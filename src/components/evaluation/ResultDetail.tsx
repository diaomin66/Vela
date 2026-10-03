import { Check, CircleAlert, Code2, FileText, Image, X } from 'lucide-react';
import { useState } from 'react';
import type { CaseResult } from '../../lib/evaluation';
import { effortLabels } from '../../lib/models';
import { Drawer } from '../Drawer';

export function EvaluationResultDetail({ result, title, onClose }: { result: CaseResult; title: string; onClose: () => void }) {
  const [tab, setTab] = useState<'result' | 'prompt' | 'output'>('result');
  return <Drawer title={title} subtitle={`${result.channelName} · ${result.modelId}`} onClose={onClose}>
    <div className="evaluation-detail-scroll">
      <div className="evaluation-result-meta"><span>{effortLabels[result.reasoningEffort ?? ''] ?? 'API 默认'}</span><span>{(result.elapsedMs / 1000).toFixed(1)} 秒</span>{result.inputTokens != null && <span>{result.inputTokens} 输入 / {result.outputTokens ?? '—'} 输出 token</span>}</div>
      <div className="evaluation-detail-tabs" role="group" aria-label="结果内容"><button className={tab === 'result' ? 'active' : ''} onClick={() => setTab('result')}><Image size={15}/>结果</button><button className={tab === 'prompt' ? 'active' : ''} onClick={() => setTab('prompt')}><FileText size={15}/>题目</button><button className={tab === 'output' ? 'active' : ''} onClick={() => setTab('output')}><Code2 size={15}/>原文</button></div>
      {tab === 'prompt' ? <pre className="evaluation-raw">{result.prompt}</pre> : tab === 'output' ? <pre className="evaluation-raw">{result.output || '本次没有返回答案。'}</pre> : <>
        {result.error && <p className="inline-error" role="alert"><CircleAlert size={15}/>{result.error}</p>}
        {result.safeSvg && <figure className="evaluation-svg"><img src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(result.safeSvg)}`} alt="被测模型绘制的骑自行车鹈鹕"/><figcaption>安全 SVG 预览 · 结构检查不评价画面质量</figcaption></figure>}
        <div className="evaluation-detail-score"><div><span>{result.caseId === 'pelican' ? 'SVG 结构分' : '本机核对分'}</span><strong>{result.score ?? '—'}<small> / {result.maxScore}</small></strong></div><span className={`evaluation-outcome ${result.status}`}>{({ passed: '检查通过', failed: '未通过', error: '请求失败', cancelled: '已取消' })[result.status]}</span></div>
        <ul className="evaluation-checks">{result.checks.map((check, i) => <li key={`${i}-${check.label}`}>{check.passed ? <Check size={16}/> : <X size={16}/>}<span>{check.label}</span></li>)}</ul>
        {!result.safeSvg && result.output && <pre className="evaluation-answer">{result.output}</pre>}
        {result.judge && <section className="evaluation-judge-result"><div><h3>{result.caseId === 'pelican' ? 'SVG 代码复评' : '模型复评'}</h3><strong>{result.judge.score == null ? '未完成' : `${result.judge.score} / 100`}</strong></div><p>{result.judge.error || result.judge.explanation}</p><small>{result.judge.modelId}{result.judge.profileId === result.profileId && result.judge.modelId === result.modelId ? ' · 同模型自评' : ''}</small></section>}
      </>}
    </div>
  </Drawer>;
}
