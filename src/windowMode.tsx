import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';

import { SetWindowMode, type WindowMode } from './api';

/** How long the stage fade-out gets before the native resize fires — matches
 *  the `#root` opacity transition in styles.css so the reflow happens behind
 *  an opaque veil. */
const SWITCH_FADE_MS = 150;

interface WindowModeCtx {
  mode: WindowMode;
  /** True while a switch is in flight (both directions). */
  switching: boolean;
  setMode: (mode: WindowMode) => void;
}

const Ctx = createContext<WindowModeCtx | null>(null);

function prefersReducedMotion(): boolean {
  return window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
}

const wait = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms));

/** One window, two shapes. The backend owns the native resize; this provider
 *  owns the React state and the `data-window-mode` stamp the CSS keys on. */
export function WindowModeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<WindowMode>('compact');
  const [switching, setSwitching] = useState(false);
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const inFlight = useRef(false);

  // Stamp the mode on <html> so styles.css can key the layout on it.
  useEffect(() => {
    document.documentElement.dataset.windowMode = mode;
    return () => {
      delete document.documentElement.dataset.windowMode;
    };
  }, [mode]);

  const setMode = useCallback((next: WindowMode) => {
    if (inFlight.current || next === modeRef.current) return;
    inFlight.current = true;
    setSwitching(true);
    const root = document.documentElement;
    const animate = !prefersReducedMotion();
    void (async () => {
      try {
        if (animate) {
          // Veil the stage, and give the fade time to finish before the native
          // window snaps to its new frame.
          root.dataset.windowModeSwitching = 'true';
          await wait(SWITCH_FADE_MS);
        }
        const report = await SetWindowMode(next);
        // Stamp synchronously with the React state so the un-veil below never
        // shows a frame of the old layout in the new window frame.
        root.dataset.windowMode = report.mode;
        setModeState(report.mode);
      } catch (cause) {
        console.warn('[window-mode] switch failed', cause);
      } finally {
        if (animate) {
          window.requestAnimationFrame(() => {
            window.requestAnimationFrame(() => {
              delete root.dataset.windowModeSwitching;
            });
          });
        }
        inFlight.current = false;
        setSwitching(false);
      }
    })();
  }, []);

  const value = useMemo(() => ({ mode, switching, setMode }), [mode, switching, setMode]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

/** Nullable accessor: chrome that merely *reacts* to the mode renders nothing
 *  without a provider (tests, standalone harnesses). */
export function useWindowModeOptional(): WindowModeCtx | null {
  return useContext(Ctx);
}

export function useWindowMode(): WindowModeCtx {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error('useWindowMode must be used within WindowModeProvider');
  return ctx;
}
