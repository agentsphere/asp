import { useEffect, useRef } from 'preact/hooks';
import { createPortal } from 'preact/compat';

interface Props {
  open: boolean;
  onClose: () => void;
  title?: string;
  children: any;
}

/**
 * Full-page overlay that renders via portal to document.body.
 * Covers most of the viewport, click outside (backdrop) closes.
 * Used for issue detail, pipeline detail, UI preview lightbox.
 */
export function Overlay({ open, onClose, title, children }: Props) {
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', handleKey);
    document.body.style.overflow = 'hidden';
    return () => {
      document.removeEventListener('keydown', handleKey);
      document.body.style.overflow = '';
    };
  }, [open, onClose]);

  if (!open) return null;

  const handleBackdropClick = (e: Event) => {
    if (panelRef.current && !panelRef.current.contains(e.target as Node)) {
      onClose();
    }
  };

  return createPortal(
    <div class="overlay-backdrop" onClick={handleBackdropClick}>
      <div class="overlay-panel" ref={panelRef}>
        {title && (
          <div class="overlay-header">
            <h2 class="overlay-title">{title}</h2>
            <button class="overlay-close" onClick={onClose}>&times;</button>
          </div>
        )}
        <div class="overlay-body">
          {children}
        </div>
      </div>
    </div>,
    document.body,
  );
}
