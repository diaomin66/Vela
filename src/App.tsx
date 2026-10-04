import { Activity, CircleAlert, CircleCheck, FlaskConical, History, Layers3, Link2, LoaderCircle, Minus, Settings2, Square, X } from 'lucide-react';
import { useRef } from 'react';
import { desktop } from './lib/api';
import { errorMessage } from './lib/utils';
import { BrandMark } from './components/Icons';
import { ModelLibrary } from './components/channels';
import { WorkspaceModals } from './components/WorkspaceModals';
import { ChannelsPage } from './pages/ChannelsPage';
import { DiagnosticsPage } from './pages/DiagnosticsPage';
import { RecoveryPage } from './pages/RecoveryPage';
import { EvaluationPage } from './pages/EvaluationPage';
import { useWorkspace } from './hooks/useWorkspace';
import { UpdateBadge } from './components/UpdatePanel';
import type { View } from './types';
import { APP_NAME } from './lib/brand';

const navigation = [{ id: 'connections', label: '渠道', icon: Link2 }, { id: 'models', label: '模型库', icon: Layers3 }, { id: 'evaluations', label: '评测', icon: FlaskConical }, { id: 'diagnostics', label: '诊断', icon: Activity }, { id: 'recovery', label: '恢复', icon: History }] as const;

function WindowControls() {
  if (!desktop) return null;
  async function control(action: 'minimize' | 'toggleMaximize' | 'close') { const { getCurrentWindow } = await import('@tauri-apps/api/window'); await getCurrentWindow()[action](); }
  return <div className="window-controls"><button aria-label="最小化窗口" onClick={() => void control('minimize')}><Minus size={15}/></button><button aria-label="最大化或还原窗口" onClick={() => void control('toggleMaximize')}><Square size={12}/></button><button aria-label="关闭窗口" onClick={() => void control('close')}><X size={16}/></button></div>;
}

export default function App() {
  const w = useWorkspace();
  const main = useRef<HTMLElement>(null);
  function navigate(view: View) { w.navigate(view); main.current?.scrollTo({ top: 0 }); }
  return <div className="app-shell">
    <header className="app-header" data-tauri-drag-region>
      <button className="brand" aria-label={`${APP_NAME} 主页`} onClick={() => navigate('connections')}><BrandMark/><span>{APP_NAME}</span></button>
      <nav className="navigation" aria-label="主导航">{navigation.map(({ id, label, icon: Icon }) => <button key={id} aria-current={w.view === id ? 'page' : undefined} className={w.view === id ? 'nav-active' : ''} onClick={() => navigate(id)}><Icon size={17}/>{label}</button>)}</nav>
      <div className="header-right"><button className="icon-button settings-button" aria-label={`${APP_NAME} 设置`} onClick={() => w.setModal({ type: 'settings' })}><Settings2 size={19}/></button><WindowControls/></div>
    </header>
    <main ref={main} className="main-content" id="workspace" tabIndex={-1}>
      {!w.data ? <div className="loading-page">{w.loadError ? <><CircleAlert size={27}/><h1>无法读取配置</h1><p>{w.loadError}</p><button className="button button-primary" onClick={() => void w.refresh().catch((err: unknown) => w.notify(errorMessage(err), true))}>重试</button></> : <LoaderCircle className="spin" size={27}/>}</div> : <>
        {w.view === 'connections' && <ChannelsPage workspace={w}/>}
        {w.view === 'models' && <ModelLibrary data={w.data} busy={w.busy} onApply={(routeId) => void w.prepareGateway(routeId)} onEdit={w.openEditor} onValidate={w.openValidation} onAdd={() => w.openEditor()} onReasoningChange={w.setModelReasoning}/>}
        {w.view === 'evaluations' && <EvaluationPage workspace={w.data}/>}
        {w.view === 'diagnostics' && <DiagnosticsPage workspace={w}/>}
        {w.view === 'recovery' && <RecoveryPage backups={w.data.backups} busy={w.busy} onRestore={(backup) => void w.prepareRestore(backup)}/>}
      </>}
    </main>
    <footer className="app-footer"><span className="footer-status"><span className={`status-dot ${w.data?.gateway.running ? 'online' : 'offline'}`}/>{desktop ? w.data?.gateway.running ? '后台运行中' : '后台未就绪' : '演示模式 · 不修改本机配置'}</span><UpdateBadge onOpen={() => w.setModal({ type: 'settings' })}/><button onClick={() => w.setModal({ type: 'settings' })}>{APP_NAME} {w.data?.environment.appVersion ?? '0.7.0'}</button></footer>
    <WorkspaceModals workspace={w}/>
    {w.toast && <div className={`toast ${w.toast.error ? 'toast-error' : ''}`} role={w.toast.error ? 'alert' : 'status'}>{w.toast.error ? <CircleAlert size={18}/> : <CircleCheck size={18}/>}<span>{w.toast.message}</span><button className="icon-button" aria-label="关闭提示" onClick={() => w.setToast(null)}><X size={15}/></button></div>}
  </div>;
}
