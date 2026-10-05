import type { ReactNode } from 'react';

interface PageHeaderProps {
  title: string;
  description?: string;
  count?: number;
  actions?: ReactNode;
  className?: string;
}

/** One hierarchy for every workspace: name, optional scope, then actions. */
export function PageHeader({ title, description, count, actions, className = '' }: PageHeaderProps) {
  return <header className={`page-heading ${className}`}>
    <div className="page-heading-copy"><div className="heading-inline"><h1>{title}</h1>{count != null && <span className="count-badge">{count}</span>}</div>{description && <p className="page-description">{description}</p>}</div>
    {actions && <div className="page-actions">{actions}</div>}
  </header>;
}

export function SectionHeading({ title, description, actions }: { title: string; description?: string; actions?: ReactNode }) {
  return <div className="section-heading"><div><h2>{title}</h2>{description && <p className="section-description">{description}</p>}</div>{actions}</div>;
}

export function WorkspaceToolbar({ children, className = '' }: { children: ReactNode; className?: string }) {
  return <div className={`workspace-toolbar ${className}`}>{children}</div>;
}
