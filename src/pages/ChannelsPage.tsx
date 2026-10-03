import { Plus } from 'lucide-react';
import { ChannelCard } from '../components/channels';
import { ConnectionStatus } from '../components/ConnectionStatus';
import type { WorkspaceController } from '../hooks/useWorkspace';

export function ChannelsPage({ workspace: w }: { workspace: WorkspaceController }) {
  const data = w.data!;
  return <div className="view-enter"><div className="page-heading"><div className="heading-inline"><h1>渠道管理</h1><span className="count-badge">{data.profiles.length}</span></div><button className="button button-primary" onClick={() => w.openEditor()}><Plus size={16}/>添加渠道</button></div>
    <ConnectionStatus data={data} busy={w.busy} onApply={() => void w.prepareGateway()} onOpen={() => void w.openCodex()}/>
    <div className="channels-grid">{data.profiles.map((profile, index) => <ChannelCard key={profile.id} profile={profile} index={index} syncing={w.syncingIds.has(profile.id)} isDefault={data.gatewayConfigured && data.catalog.some((entry) => entry.routeId === data.defaultRouteId && entry.profileId === profile.id)} onEdit={() => w.openEditor(profile)} onDelete={() => w.setModal({ type: 'delete', profile })} onSync={() => void w.syncChannel(profile)} onValidate={() => w.openValidation(profile)}/>)}
      {!data.profiles.length && <button className="channel-empty" onClick={() => w.openEditor()}><Plus size={24}/><strong>添加第一个渠道</strong><span>填写 API 地址和 Key</span></button>}
    </div>
  </div>;
}
