import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useState, useSyncExternalStore, type ReactNode } from 'react';

export type ThemeMode = 'system' | 'light' | 'dark';
export type ResolvedTheme = 'light' | 'dark';
const storageKey = 'vela:appearance:v1';
const systemTheme = window.matchMedia('(prefers-color-scheme: dark)');
const subscribe = (notify: () => void) => {
  systemTheme.addEventListener('change', notify);
  return () => systemTheme.removeEventListener('change', notify);
};
const getSystemTheme = () => systemTheme.matches;
const validMode = (value: unknown): ThemeMode => value === 'light' || value === 'dark' ? value : 'system';
const ThemeContext = createContext<{ mode: ThemeMode; theme: ResolvedTheme; setMode: (mode: ThemeMode) => void } | null>(null);

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, updateMode] = useState<ThemeMode>(() => validMode(document.documentElement.dataset.themeMode));
  const systemDark = useSyncExternalStore(subscribe, getSystemTheme);
  const theme: ResolvedTheme = mode === 'system' ? systemDark ? 'dark' : 'light' : mode;
  const setMode = useCallback((next: ThemeMode) => {
    updateMode(next);
    try { localStorage.setItem(storageKey, next); } catch {}
  }, []);
  useLayoutEffect(() => {
    const root = document.documentElement;
    root.dataset.themeMode = mode;
    root.dataset.theme = theme;
    root.style.colorScheme = theme;
    document.querySelector('meta[name="theme-color"]')?.setAttribute('content', theme === 'dark' ? '#17191e' : '#f5f5f6');
  }, [mode, theme]);
  useEffect(() => {
    function sync(event: StorageEvent) { if (event.key === storageKey || event.key === null) updateMode(validMode(event.newValue)); }
    window.addEventListener('storage', sync);
    return () => window.removeEventListener('storage', sync);
  }, []);
  const value = useMemo(() => ({ mode, theme, setMode }), [mode, theme, setMode]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme() {
  const value = useContext(ThemeContext);
  if (!value) throw new Error('ThemeProvider is required.');
  return value;
}
