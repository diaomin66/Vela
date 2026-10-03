import { useEffect, useRef, useState } from 'react';
import { Check, ChevronDown, Plus, Search, SlidersHorizontal, Trash2 } from 'lucide-react';
import type { CatalogEntry, ChannelModel } from '../types';
import { Select, type SelectOption } from './Select';
import { effortLabels as EFFORT_LABELS } from '../lib/models';

const EFFORTS = Object.keys(EFFORT_LABELS);
const PRESETS: SelectOption[] = [
  { value: 'auto', label: '自动识别', description: '使用模型对应的原生推理档位' },
  { value: 'off', label: '不支持推理强度' },
  { value: 'standard', label: '低 · 中 · 高' },
  { value: 'extended', label: '低 · 中 · 高 · 超高' },
  { value: 'maximum', label: '低 · 中 · 高 · 超高 · 最高' },
  { value: 'custom', label: '自定义档位' },
];
function presetFor(efforts: string[] | null | undefined): string {
  if (!efforts) return 'auto';
  if (!efforts.length) return 'off';
  if (efforts.join(',') === 'low,medium,high') return 'standard';
  if (efforts.join(',') === 'low,medium,high,xhigh') return 'extended';
  if (efforts.join(',') === 'low,medium,high,xhigh,max') return 'maximum';
  return 'custom';
}

function ModelOptions({ entry, catalogEntry, disabled, onPatch, onRemove }: { entry: ChannelModel; catalogEntry?: CatalogEntry; disabled: boolean; onPatch: (patch: Partial<ChannelModel>) => void; onRemove: () => void }) {
  const [custom, setCustom] = useState(presetFor(entry.reasoningEfforts) === 'custom');
  const automaticAtOpen = useRef(entry.reasoningEfforts == null);
  const preset = custom ? 'custom' : presetFor(entry.reasoningEfforts);
  const available = entry.reasoningEfforts ?? (automaticAtOpen.current ? catalogEntry?.supportedReasoningEfforts : undefined) ?? [];
  const effortOptions = available.map((effort) => ({ value: effort, label: EFFORT_LABELS[effort] ?? effort }));
  function changePreset(next: string) {
    setCustom(next === 'custom');
    const efforts = next === 'auto' ? null : next === 'off' ? [] : next === 'standard' ? ['low', 'medium', 'high'] : next === 'extended' ? ['low', 'medium', 'high', 'xhigh'] : next === 'maximum' ? ['low', 'medium', 'high', 'xhigh', 'max'] : entry.reasoningEfforts ?? ['low', 'medium', 'high'];
    onPatch({ reasoningEfforts: efforts, defaultReasoningEffort: efforts?.includes(entry.defaultReasoningEffort ?? '') ? entry.defaultReasoningEffort : null });
  }
  return <div className="model-option-fields">
    <div className="editor-field"><label htmlFor={`model-alias-${entry.id}`}>显示备注</label><input id={`model-alias-${entry.id}`} aria-label={`${entry.id} 的显示备注`} value={entry.alias} placeholder="可选" maxLength={80} disabled={disabled} onChange={(event) => onPatch({ alias: event.target.value })}/></div>
    <div className="editor-field"><label htmlFor={`model-efforts-${entry.id}`}>推理档位</label><Select id={`model-efforts-${entry.id}`} ariaLabel={`${entry.id} 的推理档位`} value={preset} options={PRESETS} disabled={disabled} onChange={changePreset}/></div>
    {custom && <fieldset className="effort-choices"><legend>支持的推理强度</legend>{EFFORTS.map((effort) => <label key={effort}><input type="checkbox" checked={entry.reasoningEfforts?.includes(effort) ?? false} disabled={disabled} onChange={(event) => {
      const next = EFFORTS.filter((value) => value === effort ? event.target.checked : entry.reasoningEfforts?.includes(value));
      onPatch({ reasoningEfforts: next, defaultReasoningEffort: next.includes(entry.defaultReasoningEffort ?? '') ? entry.defaultReasoningEffort : null });
    }}/><span>{EFFORT_LABELS[effort]}</span></label>)}</fieldset>}
    {effortOptions.length > 0 && <div className="editor-field"><label htmlFor={`model-default-effort-${entry.id}`}>默认强度</label><Select id={`model-default-effort-${entry.id}`} ariaLabel={`${entry.id} 的默认推理强度`} value={entry.defaultReasoningEffort ?? 'auto'} options={[{ value: 'auto', label: '模型默认' }, ...effortOptions]} disabled={disabled} onChange={(value) => onPatch({ defaultReasoningEffort: value === 'auto' ? null : value })}/></div>}
    <div className="model-option-footer"><span>{effortOptions.length ? '在 Codex 中切换推理强度' : preset === 'auto' ? automaticAtOpen.current ? '跟随模型默认' : '保存后识别原生档位' : '不提供推理强度选择'}</span><button type="button" className="text-button editor-delete" disabled={disabled} onClick={onRemove}><Trash2 size={14}/>移除模型</button></div>
  </div>;
}

export function ModelSelection({ models, model, disabled, initialModelId, catalog = [], onModels, onDefault }: { models: ChannelModel[]; model: string; disabled: boolean; initialModelId?: string; catalog?: CatalogEntry[]; onModels: (models: ChannelModel[]) => void; onDefault: (id: string) => void }) {
  const [search, setSearch] = useState('');
  const [manual, setManual] = useState('');
  const [expanded, setExpanded] = useState<string | null>(initialModelId ?? null);
  const root = useRef<HTMLDivElement>(null);
  const selected = models.filter((entry) => entry.enabled);
  const filtered = models.filter((entry) => `${entry.id} ${entry.alias}`.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase()));
  const expandedIndex = filtered.findIndex((entry) => entry.id === expanded);
  const shown = expandedIndex >= 200 ? [...filtered.slice(0, 199), filtered[expandedIndex]] : filtered.slice(0, 200);
  useEffect(() => {
    if (!initialModelId) return;
    const frame = requestAnimationFrame(() => root.current?.querySelector('[data-expanded="true"]')?.scrollIntoView({ block: 'center' }));
    return () => cancelAnimationFrame(frame);
  }, [initialModelId]);
  function patch(id: string, change: Partial<ChannelModel>) { onModels(models.map((entry) => entry.id === id ? { ...entry, ...change } : entry)); }
  function add() {
    const id = manual.trim();
    if (!id || disabled) return;
    onModels(models.some((entry) => entry.id === id) ? models.map((entry) => entry.id === id ? { ...entry, enabled: true } : entry) : [...models, { id, alias: '', enabled: true }]);
    if (!model) onDefault(id);
    setManual(''); setSearch('');
  }
  return <div ref={root} className="editor-model-selection">
    {models.length > 0 && <>
      <div className="editor-search"><Search size={16}/><input aria-label="搜索渠道模型" placeholder="搜索模型" value={search} disabled={disabled} onChange={(event) => setSearch(event.target.value)}/>{search && <button type="button" className="text-button" aria-label="清除模型搜索" onClick={() => setSearch('')}>清除</button>}</div>
      <div className="editor-selection-actions"><span>{selected.length} / {models.length} 已启用</span><button type="button" className="text-button" aria-label="选中搜索结果" disabled={disabled} onClick={() => { const found = new Set(filtered.map((entry) => entry.id)); onModels(models.map((entry) => found.has(entry.id) ? { ...entry, enabled: true } : entry)); }}>全选</button><button type="button" className="text-button" aria-label="清空选择" disabled={disabled || !selected.length} onClick={() => onModels(models.map((entry) => ({ ...entry, enabled: false })))}>清空</button></div>
      <div className="editor-model-picker">{shown.map((entry) => <div className={`editor-model-choice ${entry.enabled ? 'is-selected' : ''}`} data-expanded={expanded === entry.id} key={entry.id}>
        <div className="editor-model-main"><label className="editor-model-check"><input type="checkbox" checked={entry.enabled} aria-label={entry.id} disabled={disabled} onChange={(event) => patch(entry.id, { enabled: event.target.checked })}/><span className="editor-check-box" aria-hidden="true">{entry.enabled && <Check size={13}/>}</span><span className="editor-model-name"><strong title={entry.id}>{entry.id}</strong>{entry.alias && <small>{entry.alias}</small>}</span></label><button type="button" className={`icon-button model-options-button ${expanded === entry.id ? 'is-active' : ''}`} aria-label={`${entry.id} 的模型设置`} aria-expanded={expanded === entry.id} disabled={disabled} onClick={() => setExpanded(expanded === entry.id ? null : entry.id)}>{expanded === entry.id ? <ChevronDown size={16}/> : <SlidersHorizontal size={16}/>}</button></div>
        {expanded === entry.id && <ModelOptions entry={entry} catalogEntry={catalog.find((item) => item.modelId === entry.id)} disabled={disabled} onPatch={(change) => patch(entry.id, change)} onRemove={() => { onModels(models.filter((item) => item.id !== entry.id)); setExpanded(null); }}/>}
      </div>)}{!filtered.length && <p className="editor-model-empty">没有匹配的模型</p>}{filtered.length > shown.length && <p className="editor-model-empty">显示前 200 个结果，请搜索具体模型</p>}</div>
    </>}
    <div className="editor-add-model"><input aria-label="手动添加模型" value={manual} placeholder={models.length ? '输入模型 ID，手动添加' : '模型 ID，例如 gpt-5.4'} autoComplete="off" disabled={disabled} onChange={(event) => setManual(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); add(); } }}/><button type="button" className="button button-quiet" disabled={disabled || !manual.trim()} onClick={add}><Plus size={15}/>添加</button></div>
    {selected.length > 0 && <div className="editor-field editor-default-model"><label htmlFor="default-channel-model">渠道默认模型</label><Select id="default-channel-model" ariaLabel="渠道默认模型" value={selected.some((entry) => entry.id === model) ? model : selected[0].id} onChange={onDefault} disabled={disabled} searchable={selected.length > 6} options={selected.map((entry) => ({ value: entry.id, label: entry.alias ? `${entry.alias}（${entry.id}）` : entry.id }))}/></div>}
  </div>;
}
