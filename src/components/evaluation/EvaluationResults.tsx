import * as Tabs from '@radix-ui/react-tabs';
import { ArrowRight, Bird, Candy, CheckCheck, ChevronRight, LoaderCircle } from 'lucide-react';
import { useState } from 'react';
import type { EvaluationCaseId } from '../../lib/evaluation';
import { EvaluationGallery } from './EvaluationGallery';
import { EvaluationTimeline } from './EvaluationTimeline';
import { EvaluationToolbar, type EvaluationMode } from './EvaluationToolbar';
import { caseTitles, duration, effortName, recordTime, resultLabels, type EvaluationRecord } from './presentation';

const icons = { pelican: Bird, candy: Candy, judgment: CheckCheck };

function Answers({ records, onSelect }: { records: EvaluationRecord[]; onSelect: (record: EvaluationRecord) => void }) {
  return <div className="evaluation-answer-list" data-testid="manual-results">{records.map((record) => <button className="evaluation-answer-row" key={record.id} onClick={() => onSelect(record)}><span className={`evaluation-answer-status ${record.status}`}>{resultLabels[record.status]}</span><span className="evaluation-answer-model"><strong>{record.modelAlias || record.modelId}</strong><small>{record.channelName}{record.modelAlias && record.modelAlias !== record.modelId ? ` · ${record.modelId}` : ''}</small></span><span className="evaluation-answer-facts"><span>{effortName(record.reasoningEffort)}推理 · {duration(record.elapsedMs)}</span><time dateTime={record.createdAt}>{recordTime(record.createdAt)}</time></span><ChevronRight size={16}/></button>)}</div>;
}

export function EvaluationResults({ records, mode, caseId, onCase, loaded, refreshing, onRefresh, onSelect, onCreate }: { records: EvaluationRecord[]; mode: EvaluationMode; caseId: EvaluationCaseId; onCase: (id: EvaluationCaseId) => void; loaded: boolean; refreshing: boolean; onRefresh: () => void; onSelect: (record: EvaluationRecord) => void; onCreate: () => void }) {
  const [channel, setChannel] = useState('all');
  const selected = records.filter((record) => record.caseId === caseId).sort((a, b) => b.createdAt.localeCompare(a.createdAt));
  const currentChannel = channel === 'all' || selected.some((record) => record.profileId === channel) ? channel : 'all';
  const shown = currentChannel === 'all' ? selected : selected.filter((record) => record.profileId === currentChannel);
  const Icon = icons[caseId];
  return <Tabs.Root value={caseId} onValueChange={(value) => { onCase(value as EvaluationCaseId); setChannel('all'); }}>
    <EvaluationToolbar records={selected} channel={currentChannel} onChannel={setChannel} refreshing={refreshing} onRefresh={onRefresh}/>
    <section aria-label={mode === 'manual' ? '单次检测结果' : '定时评测结果'}>{(['pelican', 'candy', 'judgment'] as const).map((id) => <Tabs.Content value={id} key={id} className="evaluation-tab-content">
      {!loaded ? <div className="evaluation-loading"><LoaderCircle className="spin" size={24}/><span>正在读取检测记录</span></div> : shown.length ? id === 'pelican' ? <EvaluationGallery records={shown} onSelect={onSelect}/> : mode === 'scheduled' ? <EvaluationTimeline records={shown} caseId={id} onSelect={onSelect}/> : <Answers records={shown} onSelect={onSelect}/> : <div className="evaluation-empty"><span className="evaluation-empty-mark"><Icon size={31}/></span><h2>{mode === 'manual' ? `开始一次${caseTitles[caseId]}` : '让检测按计划进行'}</h2><p>{mode === 'manual' ? '选择渠道和模型，完成后在这里查看结果。' : `定时生成的${caseTitles[caseId]}记录会显示在这里。`}</p><button className="button button-soft" onClick={onCreate}>{mode === 'manual' ? '新建检测' : '配置定时计划'}<ArrowRight size={15}/></button></div>}
    </Tabs.Content>)}</section>
  </Tabs.Root>;
}
