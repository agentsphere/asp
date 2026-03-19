import { useEffect } from 'preact/hooks';

interface Props {
  open: boolean;
  onClose: () => void;
  title: string;
  children: any;
  wide?: boolean;
}

export function Modal({ open, onClose, title, children, wide }: Props) {
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div class="modal-overlay" onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div class={`modal${wide ? ' modal-wide' : ''}`}>
        <div class="modal-title">{title}</div>
        {children}
      </div>
    </div>
  );
}
