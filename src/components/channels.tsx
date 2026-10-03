import { useDeferredValue, useEffect, useMemo, useRef, useState } from 'react';
import { Activity, ArrowUpRight, Check, ChevronRight, Ellipsis, KeyRound, LoaderCircle, Plus, RefreshCw, Search, Settings2, SlidersHorizontal, Trash2, X } from 'lucide-react';
import { endpointHost, readableTime } from '../lib/utils';
import { effortLabels } from '../lib/models';
import { ConnectionStatus } from './ConnectionStatus';
import { Select } from './Select';
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
  return <article className="channel-card" ref={card}>
    <div className="channel-heading"><span className={`channel-avatar avatar-${index % 3}`}>{profile.name.slice(0, 1)}</span><div><h2>{profile.name}</h2><p title={profile.resolvedBaseUrl || profile.baseUrl}>{endpointHost(profile.baseUrl)}</p></div><button className="icon-button" aria-label={`管理${profile.name}`} aria-expanded={menu} onClick={() => setMenu(!menu)}><Ellipsis size={19}/></button></div>
    {menu && <div className="channel-actions-menu"><button onClick={() => { setMenu(false); onEdit(); }}><Settings2 size={15}/>编辑渠道</button><button onClick={() => { setMenu(false); onValidate(); }} disabled={!enabled.length}><Activity size={15}/>验证默认模型</button><button className="danger-text" disabled={isDefault} onClick={() => { setMenu(false); onDelete(); }}><Trash2 size={15}/>删除渠道</button>{isDefault && <span>请先切换默认模型</span>}</div>}
    <div className="channel-balance"><span>可用额度</span><BalanceValue balance={profile.balance}/></div>
    <div className="channel-models"><span>{enabled.length} 个模型已启用</span><KeyRound size={13} aria-label={profile.keyStored ? '密钥已保存' : '缺少密钥'}/></div>
    <div className="model-tags">{enabled.slice(0, 2).map((model) => <span key={model.id} title={model.id}>{model.alias || model.id}</span>)}{enabled.length > 2 && <span>+{enabled.length - 2}</span>}{!enabled.length && <span className="tag-muted">尚未选择模型</span>}</div>
    {profile.syncError && <p className="channel-sync-error" title={profile.syncError}>{profile.syncError}</p>}
    <div className="channel-footer"><button className="icon-button" title={profile.lastSyncedAt ? `更新于 ${readableTime(profile.lastSyncedAt)}` : '刷新模型与余额'} aria-label={`刷新${profile.name}`} onClick={onSync} disabled={syncing}>{syncing ? <LoaderCircle size={15} className="spin"/> : <RefreshCw size={15}/>}</button><span>{profile.lastSyncedAt ? readableTime(profile.lastSyncedAt) : '尚未同步'}</span><button className="text-button" onClick={onEdit}>管理模型<ChevronRight size={15}/></button></div>
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
  const groups = new Map<string, CatalogEntry[]>();
  for (const entry of filtered) { const group = groups.get(entry.profileId) ?? []; group.push(entry); groups.set(entry.profileId, group); }

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
      <button className="catalog-model" disabled={busy} onClick={() => onEdit(profile, entry.modelId)} title={entry.displayName}><strong>{model?.alias || entry.modelId}</strong>{model?.alias && <span>{entry.modelId}</span>}</button>
      <div className="model-reasoning">{entry.supportedReasoningEfforts.length ? <Select ariaLabel={`${entry.displayName} 的推理强度`} value={entry.defaultReasoningEffort ?? entry.supportedReasoningEfforts[0]} options={entry.supportedReasoningEfforts.map((effort) => ({ value: effort, label: effortLabels[effort] || effort }))} onChange={(value) => void changeReasoning(entry, value)} disabled={busy || changing !== null || !onReasoningChange}/> : <button className="reasoning-unset" disabled={busy} title={model?.reasoningEfforts?.length === 0 ? '此模型不提供推理强度选择，点击修改' : '配置该模型支持的推理档位'} onClick={() => onEdit(profile, entry.modelId)}>{model?.reasoningEfforts?.length === 0 ? '不支持' : '未设置'}<SlidersHorizontal size={13}/></button>}</div>
      <button className={`default-model-button ${selected ? 'selected' : ''}`} disabled={busy || selected} onClick={() => onApply(entry.routeId)}>{selected ? <><Check size={14}/>默认模型</> : '设为默认'}</button>
      <div className="catalog-actions"><button className="icon-button" disabled={busy} aria-label={`验证 ${entry.displayName}`} title="验证模型" onClick={() => onValidate(profile, entry.modelId)}><Activity size={16}/></button><button className="icon-button" disabled={busy} aria-label={`设置 ${entry.displayName}`} title="模型设置" onClick={() => onEdit(profile, entry.modelId)}><Settings2 size={16}/></button></div>
    </div>;
  }
  return <div className="view-enter model-library"><div className="page-heading"><div className="heading-inline"><h1>模型库</h1><span className="count-badge">{catalog.length}</span></div><button className="button button-quiet" onClick={onAdd}><Plus size={16}/>添加渠道</button></div>
    <ConnectionStatus data={data} busy={busy} onApply={() => onApply()} compact/>
    <div className="catalog-toolbar"><div className="search-input"><Search size={17}/><input aria-label="搜索全部模型" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索模型或渠道"/>{query && <button className="icon-button" aria-label="清空模型搜索" onClick={() => setQuery('')}><X size={14}/></button>}</div><Select ariaLabel="按渠道筛选" value={channel} onChange={setChannel} searchable options={[{ value: 'all', label: '全部渠道' }, ...data.profiles.map((profile) => ({ value: profile.id, label: profile.name }))]}/><span className="catalog-result-count">{filtered.length} 个模型</span></div>
    {filtered.length ? <div className="model-groups">{Array.from(groups, ([id, entries]) => <section className="model-group" key={id} aria-label={entries[0].channelName}>
      <div className="model-group-heading"><button onClick={() => onEdit(profiles.get(id)!)}><span className="channel-dot"/>{entries[0].channelName}<span>{entries.length}</span><ArrowUpRight size={13}/></button><span className="reasoning-column-label">推理强度</span><span className="default-column-label">默认模型</span><span/></div>
      <div className="catalog-list">{entries.slice(0, 200).map(modelRow)}</div>{entries.length > 200 && <p className="list-limit">显示前 200 个模型，可使用搜索缩小范围。</p>}
    </section>)}</div> : <div className="empty-state"><Search size={28}/><h2>{catalog.length ? '没有匹配的模型' : '暂无已启用模型'}</h2><p>{catalog.length ? '更换关键词或筛选条件。' : '在渠道中拉取并选择需要的模型。'}</p>{catalog.length > 0 && <button className="button button-quiet" onClick={() => { setQuery(''); setChannel('all'); }}>清除筛选</button>}</div>}
  </div>;
}
