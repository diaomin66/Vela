import { describe, expect, it } from 'vitest';
import { createPreviewLocations } from '../src/lib/locations/preview';
import { DEFAULT_LOCATION_PREFERENCES } from '../src/lib/locations/types';

describe('location changes', () => {
  it('saves pending paths without changing active paths and binds preview to the request', async () => {
    const api = createPreviewLocations();
    const original = await api.status();
    const draft = { ...DEFAULT_LOCATION_PREFERENCES, codexHome: 'D:\\Codex', evaluationsDirectory: 'D:\\Evaluations', threadProtectionDirectory: 'D:\\Protected', threadIndexDirectory: 'E:\\Index' };
    const preview = await api.preview(draft);
    expect(preview.canSave).toBe(true);
    expect(preview.resolved.threadIndexDirectory).toBe('E:\\Index');
    expect(preview.resolved.exportsDirectory).toBe('D:\\Evaluations\\exports');
    expect(preview.resolved.sqliteHome).toBe('D:\\Codex');
    expect(preview.changes.find((change) => change.key === 'codexHome')?.migration).toBe('switch');
    await expect(api.save({ ...draft, codexHome: 'E:\\Other' }, preview.expectedHash)).rejects.toThrow('重新检查');
    const saved = await api.save(draft, preview.expectedHash);
    expect(saved.active).toEqual(original.active);
    expect(saved.next).toEqual(preview.resolved);
    expect(saved.requiresRestart).toBe(true);
    expect((await api.status()).pendingPreferences).toEqual(draft);
    await expect(api.save(draft, preview.expectedHash)).rejects.toThrow('重新检查');
    const reset = await api.preview(DEFAULT_LOCATION_PREFERENCES);
    const defaults = await api.save(DEFAULT_LOCATION_PREFERENCES, reset.expectedHash);
    expect(defaults.pendingPreferences).toBeNull();
    expect(defaults.requiresRestart).toBe(false);
  });

  it('rejects relative paths, trims input, and follows protection directory when index is empty', async () => {
    const api = createPreviewLocations();
    const invalid = { ...DEFAULT_LOCATION_PREFERENCES, codexHome: 'relative-folder' };
    const rejected = await api.preview(invalid);
    expect(rejected.canSave).toBe(false);
    expect(rejected.errors.join()).toContain('绝对路径');
    await expect(api.save(invalid, rejected.expectedHash)).rejects.toThrow('绝对路径');
    const preview = await api.preview({ ...DEFAULT_LOCATION_PREFERENCES, threadProtectionDirectory: '  D:\\Thread Vault  ' });
    expect(preview.preferences.threadProtectionDirectory).toBe('D:\\Thread Vault');
    expect(preview.resolved.threadIndexDirectory).toBe('D:\\Thread Vault');
    expect(preview.canSave).toBe(true);
  });

  it('reports environment precedence and allows retrying initial status failure', async () => {
    const api = createPreviewLocations({ failStatusOnce: true, overrides: [{ key: 'codexHome', environment: 'CODEX_HOME', value: 'E:\\Portable' }] });
    await expect(api.status()).rejects.toThrow('重新读取');
    const status = await api.status();
    expect(status.active.codexHome).toBe('E:\\Portable');
    expect(status.overrides).toHaveLength(1);
    expect((await api.preview({ ...DEFAULT_LOCATION_PREFERENCES, codexHome: 'D:\\Ignored' })).resolved.codexHome).toBe('E:\\Portable');
  });
});
