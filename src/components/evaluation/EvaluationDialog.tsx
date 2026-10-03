import * as Dialog from '@radix-ui/react-dialog';
import { X } from 'lucide-react';
import type { ReactNode } from 'react';

export function EvaluationDialog({ title, subtitle, children, onClose, locked = false, className = '' }: {
  title: string; subtitle: string; children: ReactNode; onClose: () => void; locked?: boolean; className?: string;
}) {
  return <Dialog.Root open onOpenChange={(open) => { if (!open && !locked) onClose(); }}>
    <Dialog.Portal>
      <Dialog.Overlay className="evaluation-dialog-overlay"/>
      <Dialog.Content className={`evaluation-dialog ${className}`} onEscapeKeyDown={(event) => { if (locked || event.target instanceof Element && event.target.closest('.vela-select-popup')) event.preventDefault(); }} onPointerDownOutside={(event) => { if (locked) event.preventDefault(); }}>
        <header className="evaluation-dialog-heading"><div><Dialog.Title>{title}</Dialog.Title><Dialog.Description>{subtitle}</Dialog.Description></div><Dialog.Close asChild><button type="button" className="icon-button" aria-label="关闭弹窗" disabled={locked}><X size={20}/></button></Dialog.Close></header>
        {children}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>;
}
