import { invoke } from '@tauri-apps/api/core';
import { version as appVersion } from '../../package.json';
import { desktop } from './api';

export type UpdatePhase = 'idle' | 'checking' | 'latest' | 'available' | 'downloading' | 'ready' | 'installing' | 'error';
export interface UpdateStatus {
  phase: UpdatePhase;
  currentVersion: string;
  version: string | null;
  notes: string | null;
  downloadedBytes: number;
  totalBytes: number | null;
  checkedAt: string | null;
  error: string | null;
  autoDownload: boolean;
}
export interface UpdaterApi {
  status(): Promise<UpdateStatus>;
  check(): Promise<UpdateStatus>;
  download(): Promise<UpdateStatus>;
  install(): Promise<UpdateStatus>;
  preferences(autoDownload: boolean): Promise<UpdateStatus>;
}

export const initialUpdateStatus = (): UpdateStatus => ({
  phase: 'idle', currentVersion: appVersion, version: null, notes: null,
  downloadedBytes: 0, totalBytes: null, checkedAt: null, error: null, autoDownload: true,
});

const nativeUpdaterApi: UpdaterApi = {
  status: () => invoke('get_update_status'),
  check: () => invoke('check_for_updates'),
  download: () => invoke('download_update'),
  install: () => invoke('install_update'),
  preferences: (autoDownload) => invoke('set_update_preferences', { autoDownload }),
};

// The browser preview is memory-only. Its deliberately simulated release never
// contacts GitHub, writes files, or starts an installer.
export function createPreviewUpdater(now: () => number = Date.now): UpdaterApi {
  let state = initialUpdateStatus();
  let began = 0;
  const nextVersion = appVersion.replace(/\d+$/, (patch) => String(Number(patch) + 1));
  function update() {
    if (state.phase === 'checking' && now() - began >= 450) {
      state = { ...state, phase: state.version === state.currentVersion ? 'latest' : state.autoDownload ? 'downloading' : 'available', version: nextVersion, notes: '这是一条演示更新，用于预览检查、下载与安装流程。', checkedAt: new Date(now()).toISOString(), totalBytes: 14 * 1024 * 1024 };
      began = now();
    }
    if (state.phase === 'downloading') {
      const progress = Math.min(1, (now() - began) / 2400);
      state = { ...state, downloadedBytes: Math.round((state.totalBytes ?? 0) * progress), phase: progress === 1 ? 'ready' : 'downloading' };
    }
    if (state.phase === 'installing' && now() - began >= 700) {
      state = { ...state, phase: 'latest', currentVersion: nextVersion, error: null };
    }
    return { ...state };
  }
  return {
    async status() { return update(); },
    async check() {
      update();
      if (['checking', 'downloading', 'ready', 'installing'].includes(state.phase)) return { ...state };
      state = { ...state, phase: 'checking', error: null }; began = now(); return { ...state };
    },
    async download() {
      update();
      if (state.phase === 'available') { state = { ...state, phase: 'downloading' }; began = now(); }
      return { ...state };
    },
    async install() {
      update();
      if (state.phase !== 'ready') throw new Error('请先完成演示下载。');
      state = { ...state, phase: 'installing' }; began = now(); return { ...state };
    },
    async preferences(autoDownload) {
      update(); state = { ...state, autoDownload };
      if (autoDownload && state.phase === 'available') { state.phase = 'downloading'; began = now(); }
      return { ...state };
    },
  };
}

export const updaterApi = desktop ? nativeUpdaterApi : createPreviewUpdater();

export function updatePercent(status: Pick<UpdateStatus, 'downloadedBytes' | 'totalBytes'>): number | undefined {
  if (!status.totalBytes || !Number.isFinite(status.totalBytes) || status.totalBytes <= 0) return undefined;
  return Math.min(100, Math.max(0, Math.round(status.downloadedBytes / status.totalBytes * 100)));
}

export function updateSize(bytes: number): string {
  return `${(Math.max(0, bytes) / 1024 / 1024).toFixed(1)} MB`;
}
