import { ArrowRight, Link2, Plus, Search, X } from 'lucide-react';
import { useDeferredValue, useState } from 'react';
import { ChannelCard } from '../components/channels';
import { ConnectionStatus } from '../components/ConnectionStatus';
import type { WorkspaceController } from '../hooks/useWorkspace';

export function ChannelsPage({ workspace: w }: { workspace: WorkspaceController }) {
  const data = w.data!;
  const [query, setQuery] = useState('');
  const search = useDeferredValue(query.trim().toLocaleLowerCase());
  const profiles = data.profiles.filter((profile) => `${profile.name} ${profile.baseUrl} ${profile.models.map((model) => `${model.id} ${model.alias}`).join(' ')}`.toLocaleLowerCase().includes(search));
  return <div className="view-enter"><div className="page-heading"><div className="heading-inline"><h1>渠道管理</h1><span className="count-badge">{data.profiles.length}</span></div><button className="button button-primary" onClick={() => w.openEditor()}><Plus size={16}/>添加渠道</button></div>
    <ConnectionStatus data={data} busy={w.busy} onApply={() => void w.prepareGateway()} onOpen={() => void w.openCodex()}/>
    {data.profiles.length > 0 && <div className="channel-toolbar"><div className="search-input"><Search size={16}/><input aria-label="搜索渠道" placeholder="搜索渠道、地址或模型" value={query} onChange={(event) => setQuery(event.target.value)}/>{query && <button className="icon-button" aria-label="清空渠道搜索" onClick={() => setQuery('')}><X size={14}/></button>}</div><span>{profiles.length} 个渠道</span></div>}
    <div className="channels-grid">{profiles.map((profile, index) => <ChannelCard key={profile.id} profile={profile} index={index} syncing={w.syncingIds.has(profile.id)} isDefault={data.gatewayConfigured && data.catalog.some((entry) => entry.routeId === data.defaultRouteId && entry.profileId === profile.id)} onEdit={() => w.openEditor(profile)} onDelete={() => w.setModal({ type: 'delete', profile })} onSync={() => void w.syncChannel(profile)} onValidate={() => w.openValidation(profile)}/>)}
      {!data.profiles.length && <button className="channel-empty" onClick={() => w.openEditor()}><span className="channel-empty-art" aria-hidden="true"><span/><Link2 size={29}/><span/></span><strong>添加第一个渠道</strong><span>填写 API 地址和 Key，连接你的模型。</span><span className="channel-empty-action">开始连接<ArrowRight size={16}/></span></button>}
      {data.profiles.length > 0 && !profiles.length && <div className="empty-state"><Search size={26}/><h2>没有匹配的渠道</h2><p>试试渠道名称、API 地址或模型名。</p><button className="button button-quiet" onClick={() => setQuery('')}>清除筛选</button></div>}
    </div>
  </div>;
}
