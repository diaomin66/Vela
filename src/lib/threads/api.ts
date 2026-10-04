import { invoke } from '@tauri-apps/api/core';
import { desktop } from '../api';
import { createPreviewThreads } from './preview';
import type { ThreadsApi } from './types';

const native: ThreadsApi = {
  dashboard: () => invoke('get_thread_dashboard'),
  scan: () => invoke('scan_threads'),
  detail: (key) => invoke('get_thread_detail', { key }),
  previewRestore: (key) => invoke('preview_thread_restore', { key }),
  restore: (key, expectedHash) => invoke('restore_thread', { key, expectedHash }),
  settings: () => invoke('get_thread_settings'),
  saveSettings: (settings) => invoke('save_thread_settings', { settings }),
  reconcile: (sourceId) => invoke('reconcile_thread_index', { sourceId }),
  list: (query) => invoke('list_threads', { query }),
  rebuild: () => invoke('rebuild_thread_inventory'),
  open: (key) => invoke('open_thread', { key }),
};

const demo = typeof window !== 'undefined' ? new URLSearchParams(window.location.search).get('threadDemo') : null;
export const threadsApi = desktop ? native : createPreviewThreads(Date.now, demo === 'empty', demo === 'batch' ? 'batch' : 'standard');
