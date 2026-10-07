import * as Dialog from '@radix-ui/react-dialog';
import { X } from 'lucide-react';
import type { ReactNode } from 'react';

export function ThreadDialog({ title, description, children, onClose, locked = false, wide = false }: { title: string; description: string; children: ReactNode; onClose: () => void; locked?: boolean; wide?: boolean }) {
  return <Dialog.Root open onOpenChange={(open) => { if (!open && !locked) onClose(); }}><Dialog.Portal>
    <Dialog.Overlay className="thread-dialog-overlay"/>
    <Dialog.Content className={`thread-dialog${wide ? ' thread-dialog-wide' : ''}`} onEscapeKeyDown={(event) => { if (locked || event.target instanceof Element && event.target.closest('.ahax-select-popup')) event.preventDefault(); }} onPointerDownOutside={(event) => { if (locked) event.preventDefault(); }}>
      <header className="thread-dialog-heading"><div><Dialog.Title>{title}</Dialog.Title><Dialog.Description>{description}</Dialog.Description></div><Dialog.Close asChild><button className="icon-button" aria-label="关闭弹窗" disabled={locked}><X size={20}/></button></Dialog.Close></header>
      {children}
    </Dialog.Content>
  </Dialog.Portal></Dialog.Root>;
}
