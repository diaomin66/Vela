import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { threadsApi } from '../src/lib/threads/api';

vi.mock('../src/lib/api', () => ({ desktop: true }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue({ items: [], total: 0, offset: 0, limit: 20 }) }));
beforeEach(() => vi.mocked(invoke).mockClear());

describe('thread native adapter', () => {
  it('always supplies required pagination arguments, including partially specified options', async () => {
    await threadsApi.trash();
    expect(invoke).toHaveBeenLastCalledWith('list_thread_trash', { offset: 0, limit: 20 });
    await threadsApi.trash({ offset: 40 });
    expect(invoke).toHaveBeenLastCalledWith('list_thread_trash', { offset: 40, limit: 20 });
    await threadsApi.trash({ limit: 10 });
    expect(invoke).toHaveBeenLastCalledWith('list_thread_trash', { offset: 0, limit: 10 });
  });
});
