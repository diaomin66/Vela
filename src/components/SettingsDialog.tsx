import { useState, type FormEvent } from 'react';
import { Check, ChevronDown, LoaderCircle, Monitor, Moon, Sun } from 'lucide-react';
import { api } from '../lib/api';
import { errorMessage } from '../lib/utils';
import type { Dashboard } from '../types';
import { Drawer } from './Drawer';
import { Select } from './Select';
import { UpdatePanel } from './UpdatePanel';
import { useTheme, type ThemeMode } from '../lib/theme';
import { APP_NAME } from '../lib/brand';

const REFRESH_INTERVALS = [5, 15, 30, 60, 120, 360, 1440];
const appearanceOptions = [{ value: 'system', label: '跟随系统', icon: Monitor }, { value: 'light', label: '浅色', icon: Sun }, { value: 'dark', label: '深色', icon: Moon }] as const;

export function SettingsDialog({ data, onClose, onSaved }: { data: Dashboard; onClose: () => void; onSaved: () => Promise<void> }) {
  const appearance = useTheme();
  const [settings, setSettings] = useState(data.settings);
  const [busy, setBusy] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [customRefresh, setCustomRefresh] = useState(!REFRESH_INTERVALS.includes(settings.refreshMinutes));
  const [error, setError] = useState<string | null>(null);
  async function save(event: FormEvent) {
    event.preventDefault(); setBusy(true); setError(null);
    try { await api.saveSettings(settings); await onSaved(); }
    catch (err) { setError(errorMessage(err)); }
    finally { setBusy(false); }
  }
  const changedConnection = settings.providerName !== data.settings.providerName || settings.gatewayPort !== data.settings.gatewayPort;
  return <Drawer title={`${APP_NAME} 设置`} onClose={onClose} locked={busy}>
    <form className="editor-form" onSubmit={save}>
      <div className="editor-scroll">
        <section className="editor-section appearance-section">
          <fieldset className="appearance-options"><legend>外观</legend>{appearanceOptions.map(({ value, label, icon: Icon }) => <label key={value}><input type="radio" name="appearance" value={value} checked={appearance.mode === value} onChange={() => appearance.setMode(value as ThemeMode)}/><span><Icon size={19}/><strong>{label}</strong></span></label>)}</fieldset>
        </section>
        <section className="editor-section" aria-label="常规设置">
          <div className="editor-field"><label htmlFor="provider-name">服务商显示名称</label><input id="provider-name" value={settings.providerName} onChange={(event) => setSettings({ ...settings, providerName: event.target.value })} maxLength={40} required disabled={busy}/><p className="editor-hint">Codex 中的统一服务商入口。</p></div>
          <label className="editor-settings-toggle" htmlFor="auto-refresh"><span>自动同步模型与余额</span><input id="auto-refresh" type="checkbox" checked={settings.autoRefresh} onChange={(event) => setSettings({ ...settings, autoRefresh: event.target.checked })} disabled={busy}/><span aria-hidden="true"/></label>
          {settings.autoRefresh && <div className="editor-field" style={{ marginTop: 15 }}><label htmlFor="refresh-interval">同步间隔</label><Select id="refresh-interval" ariaLabel="同步间隔" value={customRefresh ? 'custom' : String(settings.refreshMinutes)} disabled={busy} options={[...REFRESH_INTERVALS.map((value) => ({ value: String(value), label: value < 60 ? `${value} 分钟` : value === 1440 ? '每天' : `${value / 60} 小时` })), { value: 'custom', label: '自定义' }]} onChange={(value) => { setCustomRefresh(value === 'custom'); if (value !== 'custom') setSettings({ ...settings, refreshMinutes: Number(value) }); }}/>{customRefresh && <div className="editor-field"><label htmlFor="refresh-minutes">同步间隔（分钟）</label><input id="refresh-minutes" type="number" min={5} max={1440} value={settings.refreshMinutes} onChange={(event) => setSettings({ ...settings, refreshMinutes: event.target.valueAsNumber })} required disabled={busy}/></div>}</div>}
          <p className="editor-settings-note"><Monitor size={16}/><span>关闭窗口后继续在后台运行，可从系统托盘完全退出。</span></p>
        </section>
        <UpdatePanel/>
        <section className="editor-section editor-advanced-section">
          <button type="button" className="editor-disclosure" aria-expanded={advanced} aria-controls="advanced-settings" onClick={() => setAdvanced(!advanced)}>连接与存储<ChevronDown size={16} className={advanced ? 'is-open' : ''}/></button>
          {advanced && <div id="advanced-settings" className="editor-advanced-fields">
            <div className="editor-field"><label htmlFor="gateway-port">本地服务端口</label><input id="gateway-port" type="number" min={1024} max={65535} value={settings.gatewayPort} onChange={(event) => setSettings({ ...settings, gatewayPort: event.target.valueAsNumber })} required disabled={busy}/></div>
            <div className="editor-settings-path"><span>配置文件</span><code>{data.environment.configPath}</code></div>
            <p className="editor-hint" style={{ marginTop: 14 }}>密钥保存在 Windows 凭据管理器。卸载前，可在「恢复」中还原接入前的配置。</p>
          </div>}
        </section>
      </div>
      <footer className="editor-footer">
        {changedConnection && <p className="editor-demo-note">保存后需重新应用 Codex 配置。</p>}
        {error && <div className="inline-error" role="alert">{error}</div>}
        <div className="editor-footer-actions"><span className="editor-footer-status"/><button type="button" className="button button-quiet" disabled={busy} onClick={onClose}>取消</button><button type="submit" className="button button-primary" disabled={busy}>{busy ? <LoaderCircle className="spin" size={16}/> : <Check size={16}/>}保存设置</button></div>
      </footer>
    </form>
  </Drawer>;
}
