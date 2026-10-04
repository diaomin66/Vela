import { Trash2 } from 'lucide-react';
import { EvaluationDialog } from './EvaluationDialog';

export function EvaluationDeleteDialog({ count, pending, error, onCancel, onConfirm }: { count: number; pending: boolean; error: string | null; onCancel: () => void; onConfirm: () => void }) {
  return <EvaluationDialog title={count === 1 ? '删除这轮评测？' : `删除 ${count} 轮评测？`} subtitle="此操作无法撤销" onClose={onCancel} locked={pending} className="evaluation-delete-dialog">
    <div className="evaluation-delete-body"><span className="evaluation-delete-mark"><Trash2 size={23}/></span><p>将删除所选评测的全部模型回答、作品和报告记录。已导出的文件会保留，定时计划不受影响。</p>{error && <p className="inline-error" role="alert">{error}</p>}</div>
    <footer className="evaluation-dialog-actions"><button className="button button-quiet" disabled={pending} onClick={onCancel}>取消</button><button className="button evaluation-danger-button" disabled={pending} onClick={onConfirm}>{pending ? '正在删除…' : '确认删除'}</button></footer>
  </EvaluationDialog>;
}
