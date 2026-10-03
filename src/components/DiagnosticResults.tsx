import { CircleAlert, CircleCheck, Info } from 'lucide-react';
import type { DiagnosticItem } from '../types';

export function DiagnosticResults({ items }: { items: DiagnosticItem[] }) {
  return <div className="diagnostic-results">{items.map((item) => {
    const Icon = item.status === 'passed' ? CircleCheck : item.status === 'info' ? Info : CircleAlert;
    return <div className="diagnostic-result" key={item.id}>
      <span className={`result-icon ${item.status}`}><Icon size={18}/></span>
      <div><h3>{item.title}</h3><p>{item.description}</p>{item.action && <p className="result-action">{item.action}</p>}</div>
      <span className={`result-label ${item.status}`}>{({ passed: '通过', warning: '提醒', error: '需处理', info: '提示' })[item.status]}</span>
    </div>;
  })}</div>;
}
