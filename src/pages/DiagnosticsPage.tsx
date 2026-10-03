import { Activity, ArrowDownToLine, FileCheck2, KeyRound, LoaderCircle, Settings2 } from 'lucide-react';
import { DiagnosticResults } from '../components/DiagnosticResults';
import { downloadReport, readableTime } from '../lib/utils';
import type { WorkspaceController } from '../hooks/useWorkspace';

export function DiagnosticsPage({ workspace: w }: { workspace: WorkspaceController }) {
  const problems = w.report?.items.filter((item) => item.status === 'warning' || item.status === 'error').length ?? 0;
  return <div className="view-enter">
    <div className="page-heading"><h1>诊断与修复</h1>{w.report && <button className="button button-quiet" onClick={() => downloadReport(w.report)}><ArrowDownToLine size={16}/>导出报告</button>}</div>
    <section className="diagnosis-overview">
      <span className={`diagnosis-symbol ${problems ? 'warning' : ''}`}><Activity size={27}/></span>
      <div><h2>{w.diagnosing ? '正在检查' : w.report ? problems ? `${problems} 项需要处理` : '检查完成' : '检查本地配置'}</h2><p>{w.report ? w.report.summary : '检查配置、凭据与后台服务，不调用模型 API。'}</p></div>
      <div className="diagnosis-actions"><button className="button button-primary" onClick={() => w.diagnosing ? w.cancelDiagnosis() : void w.diagnose()}>{w.diagnosing && <LoaderCircle className="spin" size={16}/>} {w.diagnosing ? '取消检查' : w.report ? '重新检查' : '开始检查'}</button>{w.report?.canRepair && <button className="button button-soft" disabled={w.busy} onClick={() => void w.prepareRepair()}><Settings2 size={16}/>预览修复</button>}</div>
    </section>
    {w.report ? <section className="results-section"><div className="section-heading"><h2>检查结果</h2><span className="muted-small">{readableTime(w.report.createdAt)}</span></div><DiagnosticResults items={w.report.items}/></section> : <div className="diagnostic-scope">{[{ icon: FileCheck2, title: '配置文件', text: '语法、模型引用与目录' }, { icon: KeyRound, title: '凭据', text: '本机 Key 与读取程序' }, { icon: Activity, title: '后台服务', text: '网关状态与配置同步' }].map(({ icon: Icon, title, text }) => <div key={title}><Icon size={20}/><strong>{title}</strong><span>{text}</span></div>)}</div>}
  </div>;
}
