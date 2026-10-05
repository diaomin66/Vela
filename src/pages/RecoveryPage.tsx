import { Clock3, History, RotateCcw } from 'lucide-react';
import type { Backup } from '../types';
import { readableTime } from '../lib/utils';
import { PageHeader } from '../components/ui/Workspace';

export function RecoveryPage({ backups, busy, onRestore }: { backups: Backup[]; busy: boolean; onRestore: (backup: Backup) => void }) {
  return <div className="view-enter"><PageHeader title="配置恢复" count={backups.length} description="每次应用前自动备份，可预览差异后恢复。"/>
    {backups.length ? <div className="timeline">{backups.map((backup, index) => <article className="timeline-entry" key={backup.id}>
      <span className="timeline-point"><Clock3 size={19}/></span><div className="timeline-content"><div className="timeline-meta"><time dateTime={backup.createdAt}>{readableTime(backup.createdAt)}</time>{index === 0 && <span className="pill">最近一次</span>}</div><h2>{backup.reason}</h2><p>{backup.summary}</p></div>
      <button className="button button-quiet" disabled={busy} onClick={() => onRestore(backup)}><RotateCcw size={15}/>恢复到此时</button>
    </article>)}</div> : <div className="empty-state"><History size={30}/><h2>暂无恢复记录</h2><p>应用或修复配置后，备份会显示在这里。</p></div>}
  </div>;
}
