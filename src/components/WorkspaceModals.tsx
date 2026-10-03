import { Activity, ArrowRight, Check, CircleCheck, LoaderCircle, ShieldCheck, Trash2 } from 'lucide-react';
import { Dialog } from './Dialog';
import { ProfileEditor } from './ProfileEditor';
import { SettingsDialog } from './SettingsDialog';
import { DiagnosticResults } from './DiagnosticResults';
import { desktop } from '../lib/api';
import { endpointHost } from '../lib/utils';
import type { WorkspaceController } from '../hooks/useWorkspace';

export function WorkspaceModals({ workspace: w }: { workspace: WorkspaceController }) {
  const modal = w.modal;
  if (!modal) return null;
  if (modal.type === 'editor') return <ProfileEditor profile={modal.profile} initialModelId={modal.initialModelId} catalog={w.data?.catalog} onClose={w.closeModal} onChanged={w.refresh} onSaved={async () => { await w.refresh(); w.closeModal(); w.notify('渠道已保存'); }}/>;
  if (modal.type === 'settings') return w.data && <SettingsDialog data={w.data} onClose={w.closeModal} onSaved={async () => { await w.refresh(); w.closeModal(); w.notify('设置已保存'); }}/>;
  if (modal.type === 'delete') return <Dialog title="删除渠道" onClose={w.closeModal} locked={w.busy}>
    <p className="dialog-intro">删除「{modal.profile.name}」及其 Key。引用此渠道的历史备份可能无法恢复。</p>
    {w.modalError && <div className="inline-error" role="alert">{w.modalError}</div>}
    <div className="dialog-actions"><button className="button button-quiet" onClick={w.closeModal} disabled={w.busy}>取消</button><button className="button button-danger" onClick={() => void w.deleteProfile()} disabled={w.busy}><Trash2 size={15}/>删除渠道</button></div>
  </Dialog>;
  if (modal.type === 'change') return <Dialog title={modal.preview.title} onClose={w.closeModal} locked={w.busy} wide>
    <div className="change-list">{modal.preview.changes.map((change, index) => <div className="change-item" key={index}><h3>{change.label}</h3><div><span className="change-before">{change.before || '未设置'}</span><ArrowRight size={15}/><span className="change-after">{change.after || '移除'}</span></div></div>)}</div>
    <p className="inline-hint"><ShieldCheck size={15}/>应用前自动备份，配置发生变化时需要重新预览。</p>
    {w.modalError && <div className="inline-error" role="alert">{w.modalError}</div>}
    <div className="dialog-actions"><button className="button button-quiet" onClick={w.closeModal} disabled={w.busy}>取消</button><button className="button button-primary" onClick={() => void w.applyChange()} disabled={w.busy}>{w.busy ? <LoaderCircle className="spin" size={15}/> : <Check size={15}/>}确认并{modal.operation === 'restore' ? '恢复' : modal.operation === 'repair' ? '修复' : '应用'}</button></div>
  </Dialog>;
  return <Dialog title={w.validation ? w.validation.ok ? '验证通过' : '验证未通过' : '验证模型'} onClose={w.closeModal} wide>
    <div className="validation-profile"><div><strong>{modal.modelId ?? modal.profile.model}</strong><span>{modal.profile.name} · {endpointHost(modal.profile.baseUrl)}</span></div>{!desktop && <span className="pill">演示</span>}</div>
    {w.validating ? <div className="validation-progress" role="status"><LoaderCircle size={26} className="spin"/><p>正在检查流式响应和工具回传</p></div> : w.validation ? <DiagnosticResults items={w.validation.items}/> : <p className="dialog-intro">{desktop ? '最多发送 3 次小型请求，可能消耗渠道额度。' : '演示验证，不发送真实请求。'}结果仅适用于此模型。</p>}
    {w.modalError && <div className="inline-error" role="alert">{w.modalError}</div>}
    <div className="dialog-actions"><button className="button button-quiet" onClick={w.closeModal}>{w.validating ? '取消验证' : '关闭'}</button>{!w.validating && (w.validation?.ok ? <button className="button button-primary" onClick={() => { w.closeModal(); w.navigate('models'); }}><CircleCheck size={16}/>返回模型库</button> : <button className="button button-primary" onClick={() => void w.validate()}><Activity size={16}/>{w.validation || w.modalError ? '重新验证' : '开始验证'}</button>)}</div>
  </Dialog>;
}
