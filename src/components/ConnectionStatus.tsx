import { ArrowUpRight, Check, CircleAlert, RefreshCw } from 'lucide-react';
import type { Dashboard } from '../types';

export function ConnectionStatus({ data, busy, onApply, onOpen, compact = false }: { data: Dashboard; busy: boolean; onApply: () => void; onOpen?: () => void; compact?: boolean }) {
  const label = data.gatewayApplied ? '已同步至 Codex' : data.gatewayConfigured ? '有更改待同步' : '尚未接入 Codex';
  const selected = data.catalog.find((entry) => entry.routeId === data.defaultRouteId);
  return <div className={`connection-status ${compact ? 'compact' : ''} ${data.gatewayApplied && !data.gateway.error ? 'is-connected' : 'needs-attention'}`}>
    <span className={`status-dot ${data.gateway.running ? 'online' : 'offline'}`}/>
    <div className="connection-summary"><strong>{label}</strong>{!compact && selected && <span className="connection-detail" title={selected.displayName}>默认 · {selected.displayName}</span>}{data.gateway.error && <p className="status-error"><CircleAlert size={14}/>{data.gateway.error}</p>}</div>
    <div className="status-actions">
      {!data.gatewayApplied && <button className="button button-primary" disabled={busy || !data.catalog.some((m) => m.enabled)} onClick={onApply}>{data.gatewayConfigured ? <RefreshCw size={15}/> : <Check size={15}/>} {data.gatewayConfigured ? '更新 Codex 配置' : '接入 Codex'}</button>}
      {data.gatewayApplied && onOpen && <button className="text-button" onClick={onOpen}>打开 Codex<ArrowUpRight size={15}/></button>}
    </div>
  </div>;
}
