import { isTauri, invoke } from '@tauri-apps/api/core';
import type { Backup, ChangePreview, Dashboard, DiagnosticReport, Profile, ProfileInput, Settings, ValidationResult } from '../types';
import { createPreviewApi } from './preview';

export const desktop = isTauri();
export interface AppApi {
  dashboard(): Promise<Dashboard>;
  saveProfile(input: ProfileInput): Promise<Profile>;
  deleteProfile(id: string): Promise<void>;
  validateProfile(id: string, runId: string, modelId?: string): Promise<ValidationResult>;
  syncProfile(id: string, runId?: string): Promise<Profile>;
  saveSettings(input: Settings): Promise<Settings>;
  previewGateway(defaultRouteId?: string): Promise<ChangePreview>;
  applyGateway(defaultRouteId: string | undefined, expectedHash: string): Promise<Backup>;
  previewProfile(id: string): Promise<ChangePreview>;
  applyProfile(id: string, expectedHash: string): Promise<Backup>;
  diagnose(runId: string): Promise<DiagnosticReport>;
  cancel(runId: string): Promise<void>;
  previewRepair(): Promise<ChangePreview>;
  repair(expectedHash: string): Promise<Backup>;
  previewRestore(id: string): Promise<ChangePreview>;
  restore(id: string, expectedHash: string): Promise<Backup>;
  openCodex(): Promise<string>;
}

const nativeApi: AppApi = {
  dashboard: () => invoke('get_dashboard'),
  saveProfile: (input) => invoke('save_profile', { input }),
  deleteProfile: (id) => invoke('delete_profile', { id }),
  validateProfile: (id, runId, modelId) => invoke('validate_profile', { id, runId, modelId }),
  syncProfile: (id, runId) => invoke('sync_profile', { id, runId }),
  saveSettings: (input) => invoke('save_settings', { input }),
  previewGateway: (defaultRouteId) => invoke('preview_gateway', { defaultRouteId }),
  applyGateway: (defaultRouteId, expectedHash) => invoke('apply_gateway', { defaultRouteId, expectedHash }),
  previewProfile: (id) => invoke('preview_profile', { id }),
  applyProfile: (id, expectedHash) => invoke('apply_profile', { id, expectedHash }),
  diagnose: (runId) => invoke('run_diagnostics', { runId, includeNetwork: false }),
  cancel: (runId) => invoke('cancel_diagnostics', { runId }),
  previewRepair: () => invoke('preview_repair'),
  repair: (expectedHash) => invoke('apply_repair', { expectedHash }),
  previewRestore: (id) => invoke('preview_restore', { id }),
  restore: (id, expectedHash) => invoke('restore_backup', { id, expectedHash }),
  openCodex: () => invoke('open_codex'),
};

export const api: AppApi = desktop ? nativeApi : createPreviewApi();
