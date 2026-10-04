import { invoke } from '@tauri-apps/api/core';
import { desktop } from '../api';
import { createPreviewLocations } from './preview';
import type { LocationsApi } from './types';

const native: LocationsApi = {
  status: () => invoke('get_location_preferences'),
  preview: (preferences) => invoke('preview_location_preferences', { preferences }),
  save: (preferences, expectedHash) => invoke('save_location_preferences', { preferences, expectedHash }),
};

const demo = typeof window === 'undefined' ? null : new URLSearchParams(window.location.search).get('locationDemo');
export const locationsApi = desktop ? native : createPreviewLocations({ failStatusOnce: demo === 'error', overrides: demo === 'override' ? [{ key: 'codexHome', environment: 'CODEX_HOME', value: 'D:\\Portable\\Codex' }] : [] });
