import { Activity, ArrowDownToLine, ChevronDown, FileCheck2, History, KeyRound, LoaderCircle, Settings2 } from 'lucide-react';
import { DiagnosticResults } from '../components/DiagnosticResults';
import { downloadReport, readableTime } from '../lib/utils';
import type { WorkspaceController } from '../hooks/useWorkspace';
import './diagnostics-page.css';

export function DiagnosticsPage({ workspace: w }: { workspace: WorkspaceController }) {
  const issues = w.report?.items.filter((item) => item.status === 'warning' || item.status === 'error') ?? [];
  const verified = w.report?.items.filter((item) => item.status === 'passed') ?? [];
  const notices = w.report?.items.filter((item) => item.status === 'info' && item.id !== 'inspection-scope') ?? [];
  const problems = issues.length;
  return <div className="view-enter diagnostics-page">
    <div className="page-heading"><div><h1>诊断与修复</h1><p className="diagnostics-lead">检查本机状态，定位配置问题。</p></div>{w.report && <button className="button button-quiet" onClick={() => downloadReport(w.report)}><ArrowDownToLine size={16}/>导出脱敏报告</button>}</div>
    <section className="diagnosis-overview">
      <span className={`diagnosis-symbol ${problems ? 'warning' : ''}`}><Activity size={27}/></span>
      <div aria-live="polite"><h2>{w.diagnosing ? '正在检查' : w.report ? problems ? `${problems} 项需要处理` : '检查完成' : '检查本地配置'}</h2><p>{w.diagnosing ? '正在核对配置、模型目录与恢复点。' : w.report ? `${verified.length} 项通过 · ${problems} 项待处理 · 未调用模型 API` : '配置、凭据、模型目录与恢复点一次检查。'}</p></div>
      <div className="diagnosis-actions"><button className="button button-primary" disabled={w.busy} onClick={() => w.diagnosing ? w.cancelDiagnosis() : void w.diagnose()}>{w.diagnosing && <LoaderCircle className="spin" size={16}/>} {w.diagnosing ? '取消检查' : w.report ? '重新检查' : '开始检查'}</button>{w.report?.canRepair && <button className="button button-soft" disabled={w.busy || w.diagnosing} onClick={() => void w.prepareRepair()}><Settings2 size={16}/>预览修复</button>}</div>
    </section>
    {w.report ? <section className="results-section">
      <div className="section-heading"><h2>检查结果</h2><span className="muted-small">{readableTime(w.report.createdAt)}</span></div>
      {issues.length > 0 && <DiagnosticResults items={issues}/>}
      {notices.length > 0 && <div className="diagnostics-notices"><DiagnosticResults items={notices}/></div>}
      {verified.length > 0 && <details className="diagnostics-verified" open={!problems && !notices.length}>
        <summary><FileCheck2 size={16}/><span>已通过的检查</span><span className="diagnostics-count">{verified.length}</span><ChevronDown size={16}/></summary>
        <DiagnosticResults items={verified}/>
      </details>}
    </section> : <div className="diagnostic-scope">{[{ icon: FileCheck2, title: '配置与模型', text: '语法、目录完整性与路由' }, { icon: KeyRound, title: '凭据与服务', text: '密钥、读取程序与后台网关' }, { icon: History, title: '备份与恢复', text: '文件状态与恢复点可用性' }].map(({ icon: Icon, title, text }) => <div key={title}><Icon size={20}/><strong>{title}</strong><span>{text}</span></div>)}</div>}
    <div className="diagnostics-recovery"><div><strong>配置恢复</strong><p>查看历史备份，预览差异后恢复。每次修复都会先备份。</p></div><button className="button button-quiet" onClick={() => w.navigate('recovery')}><History size={16}/>查看备份</button></div>
  </div>;
}
