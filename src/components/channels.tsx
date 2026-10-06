import { useDeferredValue, useEffect, useMemo, useRef, useState } from 'react';
import { Activity, Check, ChevronRight, Ellipsis, KeyRound, LoaderCircle, Plus, RefreshCw, Search, Settings2, SlidersHorizontal, Trash2, X } from 'lucide-react';
import { endpointHost, readableTime } from '../lib/utils';
import { effortLabels } from '../lib/models';
import { ConnectionStatus } from './ConnectionStatus';
import { Select } from './Select';
import { PageHeader, WorkspaceToolbar } from './ui/Workspace';
import type { BalanceSnapshot, CatalogEntry, Dashboard, Profile } from '../types';

const number = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 4 });

export function BalanceValue({ balance }: { balance?: BalanceSnapshot | null }) {
  const measured = balance?.remaining != null;
  const stale = balance?.status === 'stale';
  const label = balance?.status === 'unlimited' ? '不限额度' : balance?.status === 'disabled' ? '未查询' : balance?.status === 'unsupported' ? '暂不支持查询' : balance?.status === 'error' ? '查询失败' : '尚未查询';
  return <span className={`balance-value ${stale ? 'balance-stale' : ''}`} title={[balance?.message, balance?.checkedAt ? `查询于 ${readableTime(balance.checkedAt)}` : ''].filter(Boolean).join(' · ')}>{measured ? <><strong>{number.format(balance.remaining!)}</strong><span>{balance.unit || '额度'}</span>{stale && <em>上次结果</em>}</> : <span>{label}</span>}</span>;
}

export function ChannelCard({ profile, index, syncing, isDefault, onEdit, onDelete, onSync, onValidate }: { profile: Profile; index: number; syncing: boolean; isDefault: boolean; onEdit: () => void; onDelete: () => void; onSync: () => void; onValidate: () => void }) {
  const enabled = profile.models.filter((model) => model.enabled);
  const [menu, setMenu] = useState(false);
  const card = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!menu) return;
    const outside = (event: PointerEvent) => { if (!card.current?.contains(event.target as Node)) setMenu(false); };
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') setMenu(false); };
    document.addEventListener('pointerdown', outside); document.addEventListener('keydown', escape);
    return () => { document.removeEventListener('pointerdown', outside); document.removeEventListener('keydown', escape); };
  }, [menu]);
  return <article className={`channel-card ${isDefault ? 'channel-card-default' : ''}`} ref={card}>
    <div className="channel-heading"><span className={`channel-avatar avatar-${index % 3}`}>{profile.name.slice(0, 1)}</span><div><div className="channel-title"><h2>{profile.name}</h2></div><p title={profile.resolvedBaseUrl || profile.baseUrl}>{endpointHost(profile.baseUrl)}</p></div></div>
    <button className="icon-button channel-menu-trigger" aria-label={`管理${profile.name}`} aria-expanded={menu} onClick={() => setMenu(!menu)}><Ellipsis size={19}/></button>
    {menu && <div className="channel-actions-menu"><button onClick={() => { setMenu(false); onEdit(); }}><Settings2 size={15}/>编辑渠道</button><button onClick={() => { setMenu(false); onValidate(); }} disabled={!enabled.length}><Activity size={15}/>验证默认模型</button><button className="danger-text" disabled={isDefault} onClick={() => { setMenu(false); onDelete(); }}><Trash2 size={15}/>删除渠道</button>{isDefault && <span>请先切换默认模型</span>}</div>}
    <div className="channel-overview"><div className="channel-models"><span>已启用模型</span><strong>{enabled.length}<small>个</small></strong></div><div className="channel-balance"><span>可用额度</span><BalanceValue balance={profile.balance}/></div></div>
    <div className="model-tags">{enabled.slice(0, 2).map((model) => <span key={model.id} title={model.id}>{model.alias || model.id}</span>)}{enabled.length > 2 && <span>+{enabled.length - 2}</span>}{!enabled.length && <span className="tag-muted">尚未选择模型</span>}</div>
    {profile.syncError && <p className="channel-sync-error" title={profile.syncError}>{profile.syncError}</p>}
    <div className="channel-footer"><div className="channel-health">{isDefault && <span className="channel-default"><Check size={14}/>默认渠道</span>}{!profile.keyStored && <span className="channel-key danger-text"><KeyRound size={14}/>缺少密钥</span>}</div><button className="icon-button" title={`刷新模型与余额${profile.lastSyncedAt ? ` · 上次更新 ${readableTime(profile.lastSyncedAt)}` : ''}`} aria-label={`刷新${profile.name}`} onClick={onSync} disabled={syncing}>{syncing ? <LoaderCircle size={16} className="spin"/> : <RefreshCw size={16}/>}</button><button className="text-button" onClick={onEdit}>管理模型<ChevronRight size={15}/></button></div>
  </article>;
}

export function ModelLibrary({ data, busy, onApply, onEdit, onValidate, onAdd, onReasoningChange }: { data: Dashboard; busy: boolean; onApply: (routeId?: string) => void; onEdit: (profile: Profile, modelId?: string) => void; onValidate: (profile: Profile, modelId: string) => void; onAdd: () => void; onReasoningChange?: (entry: CatalogEntry, effort: string) => Promise<void> }) {
  const [query, setQuery] = useState('');
  const [channel, setChannel] = useState('all');
  const [changing, setChanging] = useState<string | null>(null);
  const search = useDeferredValue(query.trim().toLowerCase());
  const profiles = useMemo(() => new Map(data.profiles.map((profile) => [profile.id, profile])), [data.profiles]);
  const catalog = data.catalog.filter((entry) => entry.enabled);
  const filtered = catalog.filter((entry) => (channel === 'all' || entry.profileId === channel) && `${entry.displayName} ${entry.modelId} ${entry.channelName}`.toLowerCase().includes(search));
  const grouped = new Map<string, CatalogEntry[]>();
  for (const entry of filtered) { const group = grouped.get(entry.profileId) ?? []; group.push(entry); grouped.set(entry.profileId, group); }
  const visibleEntries = [...grouped.values()].flatMap((entries) => entries.slice(0, 200));

  async function changeReasoning(entry: CatalogEntry, effort: string) {
    if (!onReasoningChange) return;
    setChanging(entry.routeId);
    try { await onReasoningChange(entry, effort); }
    finally { setChanging(null); }
  }
  function modelRow(entry: CatalogEntry) {
    const profile = profiles.get(entry.profileId)!;
    const model = profile.models.find((item) => item.id === entry.modelId);
    const selected = entry.routeId === data.defaultRouteId;
    return <div className={`catalog-row ${selected ? 'is-default' : ''}`} key={entry.routeId} data-model={entry.modelId} data-channel={entry.profileId}>
      <button className="catalog-model" disabled={busy} onClick={() => onEdit(profile, entry.modelId)} title={entry.displayName}><strong>{model?.alias || entry.modelId}</strong><span>{entry.channelName}{model?.alias && <><span className="catalog-detail-separator">/</span>{entry.modelId}</>}</span></button>
      <div className="model-reasoning"><span className="model-field-label" aria-hidden="true">推理强度</span>{entry.supportedReasoningEfforts.length ? <Select ariaLabel={`${entry.displayName} 的推理强度`} value={entry.defaultReasoningEffort ?? entry.supportedReasoningEfforts[0]} options={entry.supportedReasoningEfforts.map((effort) => ({ value: effort, label: effortLabels[effort] || effort }))} onChange={(value) => void changeReasoning(entry, value)} disabled={busy || changing !== null || !onReasoningChange}/> : <button className="reasoning-unset" disabled={busy} title={model?.reasoningEfforts?.length === 0 ? '此模型不提供推理强度选择，点击修改' : '配置该模型支持的推理档位'} onClick={() => onEdit(profile, entry.modelId)}>{model?.reasoningEfforts?.length === 0 ? '不支持' : '未设置'}<SlidersHorizontal size={13}/></button>}</div>
      <button className={`default-model-button ${selected ? 'selected' : ''}`} disabled={busy || selected} onClick={() => onApply(entry.routeId)}>{selected ? <><Check size={14}/>默认模型</> : '设为默认'}</button>
      <div className="catalog-actions"><button className="icon-button" disabled={busy} aria-label={`验证 ${entry.displayName}`} title="验证模型" onClick={() => onValidate(profile, entry.modelId)}><Activity size={16}/></button><button className="icon-button" disabled={busy} aria-label={`设置 ${entry.displayName}`} title="模型设置" onClick={() => onEdit(profile, entry.modelId)}><Settings2 size={16}/></button></div>
    </div>;
  }
  return <div className="view-enter model-library"><PageHeader title="模型库" count={catalog.length} actions={<button className="button button-quiet" onClick={onAdd}><Plus size={18}/>添加渠道</button>}/>
    <ConnectionStatus data={data} busy={busy} onApply={() => onApply()} compact/>
    <WorkspaceToolbar className="catalog-toolbar"><div className="search-input"><Search size={17}/><input aria-label="搜索全部模型" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索模型或渠道"/>{query && <button className="icon-button" aria-label="清空模型搜索" onClick={() => setQuery('')}><X size={14}/></button>}</div><Select ariaLabel="按渠道筛选" value={channel} onChange={setChannel} searchable options={[{ value: 'all', label: '全部渠道' }, ...data.profiles.map((profile) => ({ value: profile.id, label: profile.name }))]}/><span className="catalog-result-count">{filtered.length} 个模型</span></WorkspaceToolbar>
    {filtered.length ? <section className="model-groups" aria-label="已启用模型"><div className="catalog-columns" aria-hidden="true"><span>模型与渠道</span><span className="reasoning-column-label">推理强度</span><span className="default-column-label">默认模型</span><span className="actions-column-label">操作</span></div><div className="catalog-list">{visibleEntries.map(modelRow)}</div>{visibleEntries.length < filtered.length && <p className="list-limit">每个渠道显示前 200 个模型，可使用搜索缩小范围。</p>}</section> : <div className="empty-state"><Search size={28}/><h2>{catalog.length ? '没有匹配的模型' : '暂无已启用模型'}</h2><p>{catalog.length ? '更换关键词或筛选条件。' : '在渠道中拉取并选择需要的模型。'}</p>{catalog.length > 0 && <button className="button button-quiet" onClick={() => { setQuery(''); setChannel('all'); }}>清除筛选</button>}</div>}
  </div>;
}
