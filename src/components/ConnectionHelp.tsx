import { Check, CircleHelp, Copy } from 'lucide-react';
import { useState } from 'react';
import type { CatalogEntry } from '../types';
import { errorMessage } from '../lib/utils';
import { Dialog } from './Dialog';
import { Select } from './Select';

export function ConnectionHelp({ catalog, defaultRouteId }: { catalog: CatalogEntry[]; defaultRouteId?: string | null }) {
  const [open, setOpen] = useState(false);
  const [routeId, setRouteId] = useState(defaultRouteId ?? '');
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string>();
  const models = catalog.filter((entry) => entry.enabled);
  const selected = models.find((entry) => entry.routeId === routeId) ?? models[0];
  async function copyModel() {
    if (!selected) return;
    setError(undefined);
    try { await navigator.clipboard.writeText(selected.modelId); setCopied(true); }
    catch (failure) { setError(`无法复制，请手动选中模型 ID。${errorMessage(failure)}`); }
  }
  return <>
    <button className="text-button connection-help-trigger" onClick={() => { setOpen(true); setCopied(false); setError(undefined); }}><CircleHelp size={15}/>旧会话报错</button>
    {open && <Dialog title="旧会话提示模型不可用" onClose={() => setOpen(false)}>
      <p className="dialog-intro">会话会保留创建时的服务商。只切换模型，不会同时切换服务商；把统一模型库的内部标识发给原服务商，可能出现 404。</p>
      <div className="connection-help-steps">
        <section><h3>继续原会话</h3><p>在原服务商下选择它实际支持的模型 ID。先核对渠道，再复制下方的原始 ID；内部路由标识不能作为上游模型名。</p>
          {selected && <div className="connection-help-model"><Select ariaLabel="查找原始模型 ID" value={selected.routeId} onChange={(value) => { setRouteId(value); setCopied(false); setError(undefined); }} searchable options={models.map((entry) => ({ value: entry.routeId, label: entry.displayName }))}/><div className="connection-help-copy"><code>{selected.modelId}</code><button className="icon-button" aria-label={copied ? '模型 ID 已复制' : '复制原始模型 ID'} title={copied ? '已复制' : '复制原始模型 ID'} onClick={() => void copyModel()}>{copied ? <Check size={17}/> : <Copy size={17}/>}</button></div>{copied && <span className="connection-help-feedback" role="status">模型 ID 已复制</span>}{error && <p className="inline-error" role="alert">{error}</p>}</div>}
        </section>
        <section><h3>使用 ahaX 统一模型库</h3><p>确认配置已接入，完全退出 Codex 后重新打开，再新建会话选择渠道模型。已有会话和消息记录会保留。</p></section>
      </div>
      <div className="dialog-actions"><button className="button button-primary" onClick={() => setOpen(false)}>知道了</button></div>
    </Dialog>}
  </>;
}
