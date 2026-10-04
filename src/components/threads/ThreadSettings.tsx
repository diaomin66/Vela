import { Check, LoaderCircle } from 'lucide-react';
import { useState, type FormEvent } from 'react';
import type { ThreadSettings as Settings } from '../../lib/threads/types';
import { Select } from '../Select';
import { ThreadDialog } from './ThreadDialog';

const intervals = [15, 30, 60, 120, 300, 600, 1800, 3600];

export function ThreadSettings({ initial, pending, error, onClose, onSave }: { initial: Settings; pending: boolean; error: string | null; onClose: () => void; onSave: (settings: Settings) => Promise<boolean> }) {
  const [draft, setDraft] = useState(initial);
  const [custom, setCustom] = useState(!intervals.includes(initial.intervalSeconds));
  const [validation, setValidation] = useState<string | null>(null);
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!Number.isInteger(draft.intervalSeconds) || draft.intervalSeconds < 15 || draft.intervalSeconds > 3600) { setValidation('检查间隔需为 15–3600 秒的整数。'); return; }
    setValidation(null);
    if (await onSave(draft)) onClose();
  }
  return <ThreadDialog title="线程保护设置" description="自动备份保存在本机，关闭窗口后继续运行" onClose={onClose} locked={pending}>
    <form onSubmit={(event) => void submit(event)} className="thread-settings-form">
      <div className="thread-dialog-body">
        <label className="thread-setting-toggle"><span><strong>自动保护</strong><small>定期扫描新增和变化的线程记录</small></span><input type="checkbox" checked={draft.enabled} disabled={pending} onChange={(event) => setDraft({ ...draft, enabled: event.target.checked })}/></label>
        <div className="editor-field thread-interval"><label htmlFor="thread-interval">检查间隔</label><Select id="thread-interval" ariaLabel="线程检查间隔" value={custom ? 'custom' : String(draft.intervalSeconds)} options={[...intervals.map((value) => ({ value: String(value), label: value < 60 ? `${value} 秒` : `${value / 60} 分钟` })), { value: 'custom', label: '自定义' }]} disabled={pending} onChange={(value) => { setCustom(value === 'custom'); if (value !== 'custom') setDraft({ ...draft, intervalSeconds: Number(value) }); }}/>{custom && <div className="editor-field"><label htmlFor="thread-custom-interval">间隔秒数</label><input id="thread-custom-interval" type="number" min={15} max={3600} required value={draft.intervalSeconds} disabled={pending} onChange={(event) => setDraft({ ...draft, intervalSeconds: event.target.valueAsNumber })}/></div>}</div>
        <label className="thread-setting-toggle"><span><strong>配置变更前保护</strong><small>接入渠道或修复配置前，先保存线程副本</small></span><input type="checkbox" checked={draft.protectBeforeConfigurationChange} disabled={pending} onChange={(event) => setDraft({ ...draft, protectBeforeConfigurationChange: event.target.checked })}/></label>
        <label className="thread-setting-toggle"><span><strong>包含已归档线程</strong><small>将归档记录一起纳入保护</small></span><input type="checkbox" checked={draft.includeArchived} disabled={pending} onChange={(event) => setDraft({ ...draft, includeArchived: event.target.checked })}/></label>
        {(validation || error) && <p className="inline-error" role="alert">{validation || error}</p>}
      </div>
      <footer className="thread-dialog-footer"><button type="button" className="button button-quiet" disabled={pending} onClick={onClose}>取消</button><button type="submit" className="button button-primary" disabled={pending}>{pending ? <LoaderCircle className="spin" size={16}/> : <Check size={16}/>}保存设置</button></footer>
    </form>
  </ThreadDialog>;
}
