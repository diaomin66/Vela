import { useEffect, useId, useRef, type ReactNode } from 'react';
import { X } from 'lucide-react';
import './editor.css';

export function Drawer({ title, subtitle, children, onClose, locked = false }: { title: string; subtitle?: string; children: ReactNode; onClose: () => void; locked?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  const headingId = useId();
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const element = ref.current!;
    element.showModal();
    return () => element.close();
  }, []);
  return <dialog ref={ref} className="vela-drawer" aria-labelledby={headingId}
    onCancel={(event) => { event.preventDefault(); if (!locked) closeRef.current(); }}
    onClick={(event) => {
      if (event.target !== event.currentTarget || locked) return;
      const bounds = event.currentTarget.getBoundingClientRect();
      if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) closeRef.current();
    }}>
    <header className="vela-drawer-heading"><div><h2 id={headingId}>{title}</h2>{subtitle && <p>{subtitle}</p>}</div><button type="button" className="icon-button" aria-label="关闭弹窗" disabled={locked} onClick={onClose}><X size={20}/></button></header>
    {children}
  </dialog>;
}
