export interface LocationPreferences {
  codexHome: string | null;
  sqliteHome: string | null;
  backupsDirectory: string | null;
  evaluationsDirectory: string | null;
  exportsDirectory: string | null;
  threadProtectionDirectory: string | null;
  threadIndexDirectory: string | null;
}

export interface ResolvedLocations {
  codexHome: string;
  sqliteHome: string;
  backupsDirectory: string;
  evaluationsDirectory: string;
  exportsDirectory: string;
  threadProtectionDirectory: string;
  threadIndexDirectory: string;
  configPath: string;
}

export interface LocationStatus {
  preferences: LocationPreferences;
  pendingPreferences: LocationPreferences | null;
  active: ResolvedLocations;
  next: ResolvedLocations;
  requiresRestart: boolean;
  anchorDirectory: string;
  overrides: Array<{ key: string; environment: string; value: string }>;
  error: string | null;
}

export interface LocationChange {
  key: string;
  label: string;
  currentPath: string;
  nextPath: string;
  migration: 'copy' | 'switch';
  files: number;
  bytes: number;
}

export interface LocationPreview {
  expectedHash: string;
  preferences: LocationPreferences;
  resolved: ResolvedLocations;
  changes: LocationChange[];
  warnings: string[];
  errors: string[];
  canSave: boolean;
  requiresRestart: boolean;
}

export interface LocationsApi {
  status(): Promise<LocationStatus>;
  preview(preferences: LocationPreferences): Promise<LocationPreview>;
  save(preferences: LocationPreferences, expectedHash: string): Promise<LocationStatus>;
}

export const DEFAULT_LOCATION_PREFERENCES: LocationPreferences = {
  codexHome: null,
  sqliteHome: null,
  backupsDirectory: null,
  evaluationsDirectory: null,
  exportsDirectory: null,
  threadProtectionDirectory: null,
  threadIndexDirectory: null,
};
