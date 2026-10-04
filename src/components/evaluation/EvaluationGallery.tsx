import { Bird, CircleAlert, Expand, LoaderCircle } from 'lucide-react';
import { memo, useEffect, useRef, useState } from 'react';
import { useEvaluationRun } from '../../hooks/useEvaluations';
import { ArtifactPreview } from './ArtifactPreview';
import { duration, effortName, recordTime, resultLabels, type EvaluationRecord } from './presentation';

const GalleryCard = memo(function GalleryCard({ record, onSelect }: { record: EvaluationRecord; onSelect: (record: EvaluationRecord) => void }) {
  const ref = useRef<HTMLElement>(null);
  const [visible, setVisible] = useState(false);
  const [requested, setRequested] = useState(false);
  useEffect(() => {
    const observer = new IntersectionObserver(([entry]) => { setVisible(entry.isIntersecting); if (entry.isIntersecting) setRequested(true); }, { rootMargin: '160px' });
    if (ref.current) observer.observe(ref.current);
    return () => observer.disconnect();
  }, []);
  const query = useEvaluationRun(requested ? record.runId : null);
  const result = query.data?.results.find((item) => item.profileId === record.profileId && item.modelId === record.modelId && item.caseId === record.caseId && item.reasoningEffort === record.reasoningEffort);
  const html = result?.artifactHtml ?? result?.safeSvg;
  return <article className="evaluation-gallery-card" ref={ref} data-testid="pelican-card">
    <div className="evaluation-gallery-artwork">
      {html && visible ? <ArtifactPreview html={html} title={`${record.channelName} ${record.modelId} 鹈鹕动画`}/> : <div className={`evaluation-gallery-placeholder ${record.status === 'error' ? 'error' : ''}`}>{query.isPending && requested ? <LoaderCircle size={26} className="spin"/> : record.status === 'error' ? <CircleAlert size={26}/> : <Bird size={34}/>}<span>{html ? '作品已就绪' : query.error ? '预览读取失败' : record.status === 'error' ? '这次生成未完成' : query.isPending && requested ? '正在加载作品' : '暂无可预览作品'}</span>{query.error && <button className="button button-quiet" onClick={() => void query.refetch()}>重新加载</button>}</div>}
      <button className="evaluation-gallery-expand" aria-label={`查看 ${record.modelId} 鹈鹕动画 结果`} onClick={() => onSelect(record)}><Expand size={15}/><span>查看作品</span></button>
    </div>
    <button className="evaluation-gallery-caption" onClick={() => onSelect(record)} aria-label={`${recordTime(record.createdAt)} ${record.modelId} 评测详情`}>
      <div className="evaluation-gallery-model"><strong title={record.modelId}>{record.modelAlias || record.modelId}</strong><span className="evaluation-effort">推理 {effortName(record.reasoningEffort)}</span></div>
      {record.modelAlias && record.modelAlias !== record.modelId && <span className="evaluation-gallery-model-id" title={record.modelId}>{record.modelId}</span>}
      <div className="evaluation-gallery-result"><span className={`evaluation-gallery-status ${record.status}`}>{resultLabels[record.status]}</span><span>耗时 {duration(record.elapsedMs)}</span><time dateTime={record.createdAt}>{recordTime(record.createdAt)}</time></div>
    </button>
  </article>;
});

export function EvaluationGallery({ records, onSelect }: { records: EvaluationRecord[]; onSelect: (record: EvaluationRecord) => void }) {
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const groups = new Map<string, EvaluationRecord[]>();
  for (const record of records) { const group = groups.get(record.profileId); if (group) group.push(record); else groups.set(record.profileId, [record]); }
  return <div className="evaluation-gallery" data-testid="pelican-gallery">{[...groups].map(([id, entries]) => <section className="evaluation-channel-group" key={id} aria-label={`${entries[0].channelName} 鹈鹕作品`}>
    <header className="evaluation-channel-heading"><span className="evaluation-channel-mark"><Bird size={21}/></span><div><h2>{entries[0].channelName}</h2><p>{entries.length} 个作品 · 最近 {recordTime(entries[0].createdAt)}</p></div></header>
    <div className="evaluation-gallery-grid">{(expanded.has(id) ? entries : entries.slice(0, 20)).map((record) => <GalleryCard key={record.id} record={record} onSelect={onSelect}/>)}</div>
    {entries.length > 20 && !expanded.has(id) && <button className="button button-quiet evaluation-show-more" onClick={() => setExpanded((previous) => new Set([...previous, id]))}>查看更早的 {entries.length - 20} 个作品</button>}
  </section>)}</div>;
}
