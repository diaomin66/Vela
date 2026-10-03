import { ArrowDownToLine, Check, ChevronDown, CircleAlert, LoaderCircle, RefreshCw, RotateCw } from 'lucide-react';
import { desktop } from '../lib/api';
import { readableTime } from '../lib/utils';
import { updatePercent, updateSize } from '../lib/updater';
import { useUpdater } from '../hooks/useUpdater';
import './update.css';

const labels = {
  idle: '自动检查新版本', checking: '正在检查更新', latest: '已是最新版本',
  available: '发现新版本', downloading: '正在下载更新', ready: '更新已准备好',
  installing: '正在安装更新', error: '更新未完成',
};

export function UpdatePanel() {
  const updater = useUpdater();
  const { status, pending, requestError } = updater;
  const { phase } = status;
  const error = requestError ?? status.error;
  const percent = updatePercent(status);
  const active = ['checking', 'downloading', 'installing'].includes(phase);
  const hasRelease = Boolean(status.version) && ['available', 'downloading', 'ready', 'installing'].includes(phase);
  const Icon = error ? CircleAlert : active ? LoaderCircle : phase === 'latest' ? Check : phase === 'ready' ? ArrowDownToLine : RefreshCw;
  return <section className="editor-section update-panel" aria-labelledby="software-update-heading" aria-busy={active}>
    <div className="update-heading"><h3 id="software-update-heading">软件更新</h3><span>Vela {status.currentVersion}</span></div>
    <div className={`update-status ${error ? 'has-error' : ''}`}>
      <span className="update-status-icon"><Icon size={19} className={active && !error ? 'spin' : undefined}/></span>
      <div className="update-status-copy"><strong role="status">{error ? '更新未完成' : labels[phase]}</strong><span>{hasRelease ? `${desktop ? '新版本' : '演示版本'} ${status.version}` : status.checkedAt ? `上次检查 ${readableTime(status.checkedAt)}` : '从 GitHub 获取正式版本'}</span></div>
      {(!active && phase !== 'ready' && phase !== 'available') && <button type="button" className="button button-quiet update-check" disabled={pending} onClick={() => void updater.check()}>{pending ? <LoaderCircle size={14} className="spin"/> : <RefreshCw size={14}/>} {error ? '重试' : '检查更新'}</button>}
    </div>
    {phase === 'downloading' && <div className="update-download"><progress aria-label="更新下载进度" value={percent} max={100}/><div><span>{updateSize(status.downloadedBytes)}{status.totalBytes ? ` / ${updateSize(status.totalBytes)}` : ''}</span><span>{percent === undefined ? '下载中' : `${percent}%`}</span></div></div>}
    {error && <p className="update-error" role="alert">{error}</p>}
    {hasRelease && status.notes && <details className="update-notes"><summary>版本说明<ChevronDown size={14}/></summary><p>{status.notes}</p></details>}
    {phase === 'available' && <button type="button" className="button button-primary update-action" disabled={pending} onClick={() => void updater.download()}><ArrowDownToLine size={15}/>{pending ? '正在准备下载' : '下载更新'}</button>}
    {(phase === 'ready' || phase === 'installing') && <div className="update-install"><p>安装会暂时停止本地转发，请在当前任务结束后继续。</p><button type="button" className="button button-primary update-action" disabled={pending || phase === 'installing'} onClick={() => void updater.install()}>{phase === 'installing' ? <LoaderCircle className="spin" size={15}/> : <RotateCw size={15}/>} {phase === 'installing' ? '正在安装' : '安装并重启'}</button></div>}
    <label className="editor-settings-toggle update-auto" htmlFor="auto-download-updates"><span>自动下载新版本</span><input id="auto-download-updates" type="checkbox" checked={status.autoDownload} disabled={pending || phase === 'installing'} onChange={(event) => void updater.preferences(event.target.checked)}/><span aria-hidden="true"/></label>
    <p className="update-caption">{desktop ? '后台检查与下载，安装时间由你决定。此设置即时保存。' : '演示模式 · 仅模拟更新，不下载或安装软件。'}</p>
  </section>;
}

export function UpdateBadge({ onOpen }: { onOpen: () => void }) {
  const { status } = useUpdater();
  if (!['available', 'downloading', 'ready'].includes(status.phase)) return null;
  return <button type="button" className="footer-update" onClick={onOpen}><ArrowDownToLine size={12}/>{status.phase === 'ready' ? '更新已就绪' : status.phase === 'downloading' ? '更新下载中' : '有新版本'}</button>;
}
