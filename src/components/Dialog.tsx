import { useEffect, useRef, type ReactNode } from 'react';
import { X } from 'lucide-react';

export function Dialog({ title, eyebrow, children, onClose, wide = false, locked = false }: { title: string; eyebrow?: string; children: ReactNode; onClose: () => void; wide?: boolean; locked?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const dialog = ref.current!;
    dialog.showModal();
    return () => dialog.close();
  }, []);
  return <dialog ref={ref} className={`dialog ${wide ? 'dialog-wide' : ''}`} aria-labelledby="dialog-title"
    onCancel={(e) => { e.preventDefault(); if (!locked) closeRef.current(); }}
    onClick={(e) => { if (e.target === e.currentTarget && !locked) { const r = e.currentTarget.getBoundingClientRect(); if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) closeRef.current(); } }}>
    <div className="dialog-heading"><div>{eyebrow && <p className="eyebrow">{eyebrow}</p>}<h2 id="dialog-title">{title}</h2></div><button className="icon-button" aria-label="关闭弹窗" onClick={onClose} disabled={locked}><X size={19}/></button></div>
    {children}
  </dialog>;
}
