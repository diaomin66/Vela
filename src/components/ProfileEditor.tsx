import { useEffect, useRef, useState, type FormEvent } from 'react';
import { Check, ChevronDown, Eye, EyeOff, LoaderCircle, LockKeyhole, RefreshCw } from 'lucide-react';
import { Drawer } from './Drawer';
import { Select } from './Select';
import { ModelSelection } from './ModelSelection';
import { api, desktop } from '../lib/api';
import { errorMessage, validateProfileInput } from '../lib/utils';
import { BalanceValue } from './channels';
import type { BalanceConfig, CatalogEntry, ChannelModel, Profile, ProfileInput } from '../types';

export function ProfileEditor({ profile, initialModelId, catalog = [], onClose, onChanged, onSaved }: { profile?: Profile; initialModelId?: string; catalog?: CatalogEntry[]; onClose: () => void; onChanged: () => Promise<unknown>; onSaved: (profile: Profile) => Promise<void> }) {
  const [saved, setSaved] = useState(profile);
  const [name, setName] = useState(profile?.name ?? '');
  const [baseUrl, setBaseUrl] = useState(profile?.baseUrl ?? '');
  const [models, setModels] = useState<ChannelModel[]>(profile?.models ?? []);
  const [model, setModel] = useState(profile?.model ?? '');
  const [balanceConfig, setBalanceConfig] = useState<BalanceConfig>(profile?.balanceConfig ?? { mode: 'auto' });
  const [key, setKey] = useState('');
  const [visible, setVisible] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const runRef = useRef<string | null>(null);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; if (runRef.current) void api.cancel(runRef.current).catch(() => {}); }; }, []);
  const locked = saving || syncing;
  const selected = models.filter((entry) => entry.enabled);
  async function persist() {
    const input: ProfileInput = { id: saved?.id, name, baseUrl, models, model: selected.some((entry) => entry.id === model) ? model : selected[0]?.id ?? '', balanceConfig, ...(key.trim() ? { apiKey: key.trim() } : {}) };
    const problem = validateProfileInput(input, !!saved?.keyStored);
    if (problem) throw new Error(problem);
    const result = await api.saveProfile(input);
    if (mounted.current) { setSaved(result); setBaseUrl(result.baseUrl); setKey(''); }
    await onChanged();
    return result;
  }
  async function sync() {
    setSyncing(true); setError(null);
    const runId = crypto.randomUUID(); runRef.current = runId;
    try {
      const current = await persist();
      if (!mounted.current) return;
      const result = await api.syncProfile(current.id, runId);
      if (!mounted.current || runRef.current !== runId) return;
      setSaved(result); setModels(result.models); setModel(result.model); setBaseUrl(result.baseUrl);
      await onChanged();
    } catch (err) { if (mounted.current) setError(errorMessage(err)); }
    finally { if (mounted.current) setSyncing(false); runRef.current = null; }
  }
  async function submit(event: FormEvent) {
    event.preventDefault(); setSaving(true); setError(null);
    try { const result = await persist(); await onSaved(result); }
    catch (err) { if (mounted.current) setError(errorMessage(err)); }
    finally { if (mounted.current) setSaving(false); }
  }
  return <Drawer title={profile ? '编辑渠道' : '添加渠道'} subtitle={profile?.name} onClose={onClose} locked={locked}>
    <form onSubmit={submit} className="editor-form">
      <div className="editor-scroll">
        <section className="editor-section" aria-label="基本信息">
          <div className="editor-field"><label htmlFor="profile-name">渠道名称</label><input id="profile-name" value={name} onChange={(event) => setName(event.target.value)} placeholder="例如：主力渠道" maxLength={80} autoFocus={!initialModelId} autoComplete="off" required disabled={locked}/></div>
          <div className="editor-field"><label htmlFor="profile-url">API 地址{saved && <span>已绑定</span>}</label><input id="profile-url" aria-label="API 地址" type="url" value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} placeholder="https://api.example.com" autoComplete="off" spellCheck={false} required readOnly={!!saved} disabled={locked} aria-describedby="profile-url-hint"/><p id="profile-url-hint" className="editor-hint">{saved ? '更换地址请新建渠道。' : '支持带 /v1 或不带 /v1。'}</p></div>
          <div className="editor-field"><label htmlFor="profile-key">API Key{saved?.keyStored && <span>留空保留</span>}</label><div className="editor-secret"><input id="profile-key" aria-label="API Key" type={visible ? 'text' : 'password'} value={key} onChange={(event) => setKey(event.target.value)} placeholder={saved?.keyStored ? '已安全保存' : '粘贴 API Key'} autoComplete="new-password" spellCheck={false} required={!saved?.keyStored} disabled={locked}/><button type="button" className="icon-button" aria-label={visible ? '隐藏密钥' : '显示密钥'} disabled={locked} onClick={() => setVisible(!visible)}>{visible ? <EyeOff size={17}/> : <Eye size={17}/>}</button></div></div>
        </section>
        <section className="editor-section" aria-labelledby="channel-models-title">
          <div className="editor-section-heading"><h3 id="channel-models-title">模型</h3>{syncing ? <button type="button" className="text-button" onClick={() => { if (runRef.current) void api.cancel(runRef.current).catch((err: unknown) => setError(errorMessage(err))); }}><LoaderCircle size={15} className="spin"/>取消拉取</button> : <button type="button" className="button button-soft editor-sync" disabled={locked} onClick={() => void sync()}><RefreshCw size={14}/>拉取模型与余额</button>}</div>
          {saved?.syncError && <div className="editor-notice" role="status">{saved.syncError}</div>}
          <ModelSelection models={models} model={model} disabled={locked} initialModelId={initialModelId} catalog={catalog.filter((entry) => entry.profileId === saved?.id)} onModels={setModels} onDefault={setModel}/>
        </section>
        <section className="editor-section editor-advanced-section">
          <button type="button" className="editor-disclosure" onClick={() => setAdvanced(!advanced)} aria-expanded={advanced} aria-controls="balance-settings">余额查询设置<ChevronDown size={16} className={advanced ? 'is-open' : ''}/></button>
          {advanced && <div id="balance-settings" className="editor-advanced-fields">
            <div className="editor-field"><label htmlFor="balance-mode">查询方式</label><Select id="balance-mode" ariaLabel="查询方式" value={balanceConfig.mode} onChange={(mode) => setBalanceConfig({ ...balanceConfig, mode: mode as BalanceConfig['mode'] })} disabled={locked} options={[{ value: 'auto', label: '自动识别' }, { value: 'custom', label: '自定义接口' }, { value: 'disabled', label: '不查询余额' }]}/></div>
            {balanceConfig.mode === 'custom' && <>
              <div className="editor-field"><label htmlFor="balance-path">接口路径</label><input id="balance-path" placeholder="/api/account/balance" value={balanceConfig.path ?? ''} onChange={(event) => setBalanceConfig({ ...balanceConfig, path: event.target.value })} disabled={locked}/><p className="editor-hint">以 / 开头相对于站点根路径；否则相对于 API 地址。</p></div>
              <div className="editor-field"><label htmlFor="balance-value">余额字段</label><input id="balance-value" placeholder="data.balance 或 /data/balance" value={balanceConfig.valuePath ?? ''} onChange={(event) => setBalanceConfig({ ...balanceConfig, valuePath: event.target.value })} disabled={locked}/></div>
              <div className="editor-field-columns"><div className="editor-field"><label htmlFor="balance-unit">显示单位</label><input id="balance-unit" value={balanceConfig.unit ?? ''} placeholder="额度 / USD / CNY" onChange={(event) => setBalanceConfig({ ...balanceConfig, unit: event.target.value })} disabled={locked}/></div><div className="editor-field"><label htmlFor="balance-multiplier">换算倍率</label><input id="balance-multiplier" type="number" step="any" min="0.000000001" value={balanceConfig.multiplier ?? 1} onChange={(event) => setBalanceConfig({ ...balanceConfig, multiplier: event.target.valueAsNumber })} disabled={locked}/></div></div>
            </>}
            {saved?.balance && <div className="editor-balance-status"><span>最近额度</span><BalanceValue balance={saved.balance}/></div>}
            {saved?.resolvedBaseUrl && <p className="editor-hint editor-resolved-url">服务路径：{saved.resolvedBaseUrl}</p>}
          </div>}
        </section>
      </div>
      <footer className="editor-footer">
        {error && <div className="inline-error" role="alert">{error}</div>}
        {!desktop && <p className="editor-demo-note">交互演示，请使用虚拟 Key。</p>}
        <div className="editor-footer-actions"><span className="editor-footer-status"><LockKeyhole size={14}/>{selected.length ? `${selected.length} 个模型已启用` : '密钥仅存本机'}</span><button type="button" className="button button-quiet" onClick={onClose} disabled={locked}>取消</button><button className="button button-primary" type="submit" disabled={locked}>{saving ? <LoaderCircle className="spin" size={16}/> : <Check size={16}/>}保存渠道</button></div>
      </footer>
    </form>
  </Drawer>;
}
