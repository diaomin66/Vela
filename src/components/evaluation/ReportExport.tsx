import { ArrowDownToLine, Check, Copy } from 'lucide-react';
import { useState } from 'react';
import { desktop } from '../../lib/api';
import { evaluationApi } from '../../lib/evaluation';
import { errorMessage } from '../../lib/utils';

export function ReportExport({ runId }: { runId: string }) {
  const [pending, setPending] = useState(false);
  const [path, setPath] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function exportRun() {
    setPending(true); setError(null);
    try {
      const file = await evaluationApi.export(runId);
      if (desktop) { setPath(file.path); setCopied(false); }
      else { const url = URL.createObjectURL(new Blob([file.content], { type: 'application/json;charset=utf-8' })); const link = document.createElement('a'); link.href = url; link.download = file.fileName; link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); }
    } catch (cause) { setError(errorMessage(cause)); }
    finally { setPending(false); }
  }
  async function copyPath() {
    if (!path) return;
    try { await navigator.clipboard.writeText(path); setCopied(true); setError(null); }
    catch { setError('无法自动复制，请选中路径后手动复制。'); }
  }
  return <div className="evaluation-report-export"><button className="button button-quiet" aria-label="导出评测报告" disabled={pending} onClick={() => void exportRun()}><ArrowDownToLine size={16}/>{pending ? '正在导出…' : '导出报告'}</button>{path && <div className="evaluation-export"><p role="status"><Check size={14}/>报告已保存</p><div><input aria-label="报告保存路径" value={path} readOnly onFocus={(event) => event.currentTarget.select()}/><button className="button button-quiet" onClick={() => void copyPath()}>{copied ? <Check size={14}/> : <Copy size={14}/>}{copied ? '已复制' : '复制路径'}</button></div></div>}{error && <p className="inline-error" role="alert">{error}</p>}</div>;
}
