import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { describe, expect, it } from 'vitest';

const script = readFileSync(new URL('../public/theme-init.js', import.meta.url), 'utf8');
const currentKey = 'ahax:appearance:v1';
const previousKey = 'vela:appearance:v1';

function bootstrap(values: Record<string, string>, options: { systemDark?: boolean; failWrites?: boolean; failReads?: boolean } = {}) {
  const storage = new Map(Object.entries(values));
  const root = { dataset: {} as Record<string, string>, style: {} as Record<string, string> };
  runInNewContext(script, {
    document: { documentElement: root, querySelector: () => null },
    matchMedia: () => ({ matches: options.systemDark ?? false }),
    localStorage: {
      getItem: (key: string) => { if (options.failReads) throw new Error('Unavailable'); return storage.get(key) ?? null; },
      setItem: (key: string, value: string) => { if (options.failWrites) throw new Error('Quota exceeded'); storage.set(key, value); },
      removeItem: (key: string) => storage.delete(key),
    },
  });
  return { root, storage };
}

describe('appearance bootstrap migration', () => {
  it('migrates a saved preference before removing the old key', () => {
    const { root, storage } = bootstrap({ [previousKey]: 'dark' });
    expect(root.dataset).toEqual({ theme: 'dark', themeMode: 'dark' });
    expect(storage.get(currentKey)).toBe('dark');
    expect(storage.has(previousKey)).toBe(false);
  });

  it('preserves the current preference when both keys exist', () => {
    const { root, storage } = bootstrap({ [currentKey]: 'system', [previousKey]: 'light' }, { systemDark: true });
    expect(root.dataset).toEqual({ theme: 'dark', themeMode: 'system' });
    expect(storage.get(currentKey)).toBe('system');
    expect(storage.has(previousKey)).toBe(false);
  });

  it('keeps the visible preference and recovery key if migration cannot be persisted', () => {
    const { root, storage } = bootstrap({ [previousKey]: 'dark' }, { failWrites: true });
    expect(root.dataset.theme).toBe('dark');
    expect(storage.get(previousKey)).toBe('dark');
    expect(storage.has(currentKey)).toBe(false);
  });

  it('follows the system when local storage is unavailable', () => {
    const { root } = bootstrap({}, { failReads: true, systemDark: true });
    expect(root.dataset).toEqual({ theme: 'dark', themeMode: 'system' });
    expect(root.style.colorScheme).toBe('dark');
  });

  it('does not promote malformed old data', () => {
    const { root, storage } = bootstrap({ [previousKey]: 'invalid' });
    expect(root.dataset.themeMode).toBe('system');
    expect(storage.has(currentKey)).toBe(false);
  });
});
