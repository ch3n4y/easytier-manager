import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';

export type ThemeMode = 'system' | 'light' | 'dark';
type Resolved = 'light' | 'dark';

interface ThemeCtx {
  mode: ThemeMode;
  resolved: Resolved;
  setMode: (mode: ThemeMode) => void;
}

const Ctx = createContext<ThemeCtx | null>(null);
const LS_KEY = 'et.theme';

function readMode(): ThemeMode {
  const stored = localStorage.getItem(LS_KEY);
  return stored === 'system' || stored === 'light' || stored === 'dark' ? stored : 'system';
}

function systemPrefersDark(): boolean {
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? true;
}

function resolve(mode: ThemeMode): Resolved {
  return mode === 'system' ? (systemPrefersDark() ? 'dark' : 'light') : mode;
}

/** Stamp the theme on <html> before React mounts, so a light-mode user never
 *  sees a dark first frame (the bare `:root` carries the dark tokens). */
export function applyInitialTheme(): void {
  document.documentElement.dataset.theme = resolve(readMode());
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(readMode);
  const [systemDark, setSystemDark] = useState<boolean>(systemPrefersDark);

  // Track the OS appearance so "system" keeps following it live.
  useEffect(() => {
    const mq = window.matchMedia?.('(prefers-color-scheme: dark)');
    if (!mq) return;
    const onChange = () => setSystemDark(mq.matches);
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, []);

  const resolved: Resolved = mode === 'system' ? (systemDark ? 'dark' : 'light') : mode;

  useEffect(() => {
    document.documentElement.dataset.theme = resolved;
  }, [resolved]);

  const setMode = useCallback((next: ThemeMode) => {
    localStorage.setItem(LS_KEY, next);
    // Stamp synchronously too: the effect above is passive and would land a
    // frame late, which shows as a flash of the old palette.
    document.documentElement.dataset.theme = resolve(next);
    setModeState(next);
  }, []);

  const value = useMemo(() => ({ mode, resolved, setMode }), [mode, resolved, setMode]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useTheme(): ThemeCtx {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error('useTheme must be used within ThemeProvider');
  return ctx;
}
