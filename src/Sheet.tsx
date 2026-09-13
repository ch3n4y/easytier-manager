import { useCallback, useEffect, useRef, type ReactNode } from 'react';

const FOCUSABLE =
  'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

/**
 * Centered modal on a blurred scrim. Closes on Esc or a backdrop click, traps
 * Tab inside, and hands focus back to whatever opened it.
 */
export function Sheet({
  open,
  onDismiss,
  labelledBy,
  children,
}: {
  open: boolean;
  onDismiss: () => void;
  labelledBy?: string;
  children: ReactNode;
}) {
  const sheetRef = useRef<HTMLDivElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  const openRef = useRef(open);
  openRef.current = open;

  const dismiss = useCallback(() => {
    if (openRef.current) onDismiss();
  }, [onDismiss]);

  // Remember the opener while closed; focus into the sheet once it opens.
  useEffect(() => {
    if (!open) return;
    openerRef.current = document.activeElement as HTMLElement | null;
    const id = window.requestAnimationFrame(() => {
      const sheet = sheetRef.current;
      if (!sheet) return;
      const preferred =
        sheet.querySelector<HTMLElement>('[data-autofocus]') ?? sheet.querySelector<HTMLElement>(FOCUSABLE);
      preferred?.focus({ preventScroll: true });
    });
    return () => {
      window.cancelAnimationFrame(id);
      openerRef.current?.focus?.({ preventScroll: true });
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        dismiss();
        return;
      }
      if (event.key !== 'Tab') return;
      const sheet = sheetRef.current;
      if (!sheet) return;
      const items = Array.from(sheet.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
        (el) => el.offsetParent !== null,
      );
      if (items.length === 0) return;
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement as HTMLElement | null;
      if (event.shiftKey && (active === first || !sheet.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener('keydown', onKeyDown, true);
    return () => document.removeEventListener('keydown', onKeyDown, true);
  }, [open, dismiss]);

  if (!open) return null;

  return (
    <div className="scrim" onClick={dismiss}>
      <div
        ref={sheetRef}
        className="sheet"
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        onClick={(event) => event.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}
