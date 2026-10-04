export interface ThreadSource {
  id: string;
  kind: string;
  root: string;
  displayRoot: string;
  available: boolean;
  writable: boolean;
  lastScannedAt: string | null;
  error: string | null;
}

export type SnapshotState = 'protected' | 'pending' | 'missing' | 'failed' | 'tooLarge' | 'notNeeded';

export interface ThreadSummary {
  key: string;
  sourceId: string;
  threadId: string;
  path: string;
  relativePath: string;
  archived: boolean;
  title: string | null;
  cwd: string | null;
  provider: string | null;
  sourceKind: string | null;
  createdAt: string | null;
  updatedAt: string | null;
  bytes: number;
  lineCount: number;
  indexPresent: boolean;
  integrity: string;
  snapshot: SnapshotState;
  recoverability: string;
  fingerprint: string;
  scanRevision: string;
  stateIndex?: 'indexed' | 'missing' | 'unavailable' | null;
  selectedRollout?: boolean | null;
  lastVerifiedAt?: string | null;
  protectedBytes?: number | null;
  historyBase?: { threadId: string; endOrdinalExclusive: number } | null;
}

export interface ThreadMessagePreview { role: string | null; text: string; timestamp: string | null }
export interface ThreadDetail {
  summary: ThreadSummary;
  preview: ThreadMessagePreview[];
  snapshotBytes: number | null;
  snapshotHash: string | null;
  rawAvailable: boolean;
}

export interface ThreadProtectionStatus {
  state: string;
  lastSuccessAt: string | null;
  lastAttemptAt: string | null;
  protectedCount: number;
  pendingCount: number;
  failedCount: number;
  bytesProtected: number;
  currentPath: string | null;
  error: string | null;
}

export interface ThreadDashboard {
  sources: ThreadSource[];
  threads: ThreadSummary[];
  protection: ThreadProtectionStatus;
  scannedAt: string | null;
  scanRevision: string;
  total: number;
  protected: number;
  recoverable: number;
  attention: number;
  error: string | null;
}

export interface ThreadSettings {
  enabled: boolean;
  intervalSeconds: number;
  protectBeforeConfigurationChange: boolean;
  includeArchived: boolean;
}

export interface ThreadRestorePreview {
  thread: ThreadSummary;
  snapshotHash: string;
  targetPath: string;
  targetExists: boolean;
  targetHash: string | null;
  conflict: boolean;
  expectedHash: string;
  warning: string | null;
}

export interface ThreadReconcileResult {
  sourceId: string;
  activeCount: number;
  archivedCount: number;
  completedAt: string;
  message: string;
}

export interface ThreadDeletionPreviewItem {
  key: string;
  threadId: string;
  sourceId: string;
  title?: string | null;
  rolloutCount: number;
  bytes: number;
  canDelete: boolean;
  reason?: string;
}

export interface ThreadDeletionPreview {
  expectedHash: string;
  items: ThreadDeletionPreviewItem[];
  logicalCount: number;
  rolloutCount: number;
  warning: string;
}

export type ThreadDeletionItemStatus = 'deleted' | 'blocked' | 'failed' | 'interrupted' | 'restored';
export interface ThreadDeletionResultItem {
  key: string;
  threadId: string;
  status: ThreadDeletionItemStatus;
  trashId?: string | null;
  message: string;
}

export interface ThreadDeletionResult {
  items: ThreadDeletionResultItem[];
  deletedCount: number;
  failedCount: number;
}

export interface ThreadTrashItem {
  id: string;
  threadId: string;
  sourceId: string;
  title: string | null;
  deletedAt: string;
  rolloutCount: number;
  bytes: number;
  state: 'deleted' | 'prepared' | 'interrupted' | 'restoring';
  message?: string | null;
}

export interface ThreadTrashPage {
  items: ThreadTrashItem[];
  total: number;
  offset: number;
  limit: number;
}

export interface ThreadTrashRestorePreview {
  id: string;
  threadId: string;
  rolloutCount: number;
  expectedHash: string;
  canRestore: boolean;
  reason?: string;
  warning: string;
}

export interface ThreadListQuery {
  search: string;
  scope: 'all' | 'active' | 'archived';
  status: 'all' | 'protected' | 'recoverable' | 'attention';
  sourceId: string | null;
  offset: number;
  limit: number;
}

export interface ThreadPage {
  threads: ThreadSummary[];
  total: number;
  offset: number;
  limit: number;
  scanRevision: string;
}

export interface ThreadsApi {
  dashboard(): Promise<ThreadDashboard>;
  scan(): Promise<ThreadDashboard>;
  detail(key: string): Promise<ThreadDetail>;
  previewRestore(key: string): Promise<ThreadRestorePreview>;
  restore(key: string, expectedHash: string): Promise<ThreadDashboard>;
  settings(): Promise<ThreadSettings>;
  saveSettings(settings: ThreadSettings): Promise<ThreadDashboard>;
  reconcile(sourceId: string): Promise<ThreadReconcileResult>;
  list(query: ThreadListQuery): Promise<ThreadPage>;
  rebuild(): Promise<ThreadDashboard>;
  open(key: string): Promise<string>;
  previewDeletion(keys: string[]): Promise<ThreadDeletionPreview>;
  deleteThreads(keys: string[], expectedHash: string): Promise<ThreadDeletionResult>;
  trash(query?: { offset?: number; limit?: number }): Promise<ThreadTrashPage>;
  previewTrashRestore(id: string): Promise<ThreadTrashRestorePreview>;
  restoreTrash(id: string, expectedHash: string): Promise<ThreadDeletionResult>;
}
