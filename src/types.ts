export type View = 'connections' | 'models' | 'diagnostics' | 'recovery';
export type Status = 'passed' | 'warning' | 'error' | 'info';
export type ReasoningEffort = 'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh' | 'max';
export interface ChannelModel { id: string; alias: string; enabled: boolean; reasoningEfforts?: string[] | null; defaultReasoningEffort?: string | null }
export interface BalanceConfig { mode: 'auto' | 'disabled' | 'custom'; path?: string; valuePath?: string; unit?: string; multiplier?: number }
export interface BalanceSnapshot { status: string; remaining?: number | null; unit: string; source: string; checkedAt: string; message?: string | null }
export interface Settings { providerName: string; gatewayPort: number; autoRefresh: boolean; refreshMinutes: number }
export interface CatalogEntry { routeId: string; profileId: string; channelName: string; modelId: string; displayName: string; enabled: boolean; supportedReasoningEfforts: string[]; defaultReasoningEffort: string | null }
export interface Profile {
  id: string;
  name: string;
  baseUrl: string;
  model: string;
  keyStored: boolean;
  createdAt: string;
  updatedAt: string;
  revision: string;
  lastValidatedAt?: string | null;
  models: ChannelModel[];
  resolvedBaseUrl?: string | null;
  balance?: BalanceSnapshot | null;
  balanceConfig: BalanceConfig;
  lastSyncedAt?: string | null;
  syncError?: string | null;
}
export interface ProfileInput {
  id?: string;
  name: string;
  baseUrl: string;
  model: string;
  apiKey?: string;
  models?: ChannelModel[];
  balanceConfig?: BalanceConfig;
}
export interface Backup {
  id: string;
  createdAt: string;
  reason: string;
  summary: string;
  configExisted: boolean;
}
export interface Environment {
  platform: string;
  codexInstalled: boolean;
  codexVersion?: string | null;
  configPath: string;
  configExists: boolean;
  configValid: boolean;
  appVersion: string;
  desktopMode: boolean;
}
export interface Dashboard {
  environment: Environment;
  profiles: Profile[];
  activeProfileId: string | null;
  activeProfileMatches: boolean;
  backups: Backup[];
  settings: Settings;
  gateway: { running: boolean; port: number; baseUrl: string; error?: string | null };
  catalog: CatalogEntry[];
  gatewayApplied: boolean;
  gatewayConfigured: boolean;
  defaultRouteId?: string | null;
}
export interface DiagnosticItem {
  id: string;
  category: string;
  title: string;
  status: Status;
  description: string;
  action?: string | null;
  repairable: boolean;
}
export interface DiagnosticReport {
  id: string;
  createdAt: string;
  items: DiagnosticItem[];
  summary: string;
  canRepair: boolean;
}
export interface ValidationResult {
  ok: boolean;
  checkedAt: string;
  items: DiagnosticItem[];
  latencyMs?: number | null;
  capabilities: { responses: boolean; streaming: boolean; tools: boolean };
}
export interface ChangePreview {
  id: string;
  title: string;
  summary: string;
  changes: { label: string; before: string; after: string }[];
  expectedHash: string;
  profileId?: string | null;
  backupId?: string | null;
}
