import { DEFAULT_LOCATION_PREFERENCES, type LocationPreferences, type LocationsApi, type LocationStatus, type ResolvedLocations } from './types';

const root = 'C:\\Users\\Demo';
const data = root + '\\.ahax';
const defaults: ResolvedLocations = {
  codexHome: root + '\\.codex', sqliteHome: root + '\\.codex', backupsDirectory: data + '\\backups',
  evaluationsDirectory: data + '\\evaluations', exportsDirectory: data + '\\evaluations\\exports',
  threadProtectionDirectory: data + '\\threads', threadIndexDirectory: data + '\\threads', configPath: root + '\\.codex\\config.toml',
};
const labels: Record<keyof LocationPreferences, string> = {
  codexHome: 'Codex 数据目录', sqliteHome: '索引数据库目录', backupsDirectory: '配置备份目录', evaluationsDirectory: '评测数据目录',
  exportsDirectory: '导出目录', threadProtectionDirectory: '线程保护目录', threadIndexDirectory: '线程索引目录',
};
const normalize = (input: LocationPreferences): LocationPreferences => Object.fromEntries(Object.entries(input).map(([key, value]) => [key, value?.trim() || null])) as unknown as LocationPreferences;

export function createPreviewLocations(options: { failStatusOnce?: boolean; overrides?: LocationStatus['overrides'] } = {}): LocationsApi {
  const preferences = structuredClone(DEFAULT_LOCATION_PREFERENCES);
  let pendingPreferences: LocationPreferences | null = null;
  let revision = 1;
  let failStatus = options.failStatusOnce;
  const overrides = options.overrides || [];
  function resolved(input: LocationPreferences): ResolvedLocations {
    const home = overrides.find((item) => item.key === 'codexHome')?.value || input.codexHome || defaults.codexHome;
    const evaluations = input.evaluationsDirectory || defaults.evaluationsDirectory;
    const protection = input.threadProtectionDirectory || defaults.threadProtectionDirectory;
    return {
      codexHome: home, sqliteHome: overrides.find((item) => item.key === 'sqliteHome')?.value || input.sqliteHome || home,
      backupsDirectory: input.backupsDirectory || defaults.backupsDirectory, evaluationsDirectory: evaluations,
      exportsDirectory: input.exportsDirectory || evaluations + '\\exports',
      threadProtectionDirectory: protection, threadIndexDirectory: input.threadIndexDirectory || protection,
      configPath: home + '\\config.toml',
    };
  }
  function status(): LocationStatus {
    return structuredClone({ preferences, pendingPreferences, active: resolved(preferences), next: resolved(pendingPreferences || preferences), requiresRestart: !!pendingPreferences, anchorDirectory: data, overrides, error: null });
  }
  function preview(input: LocationPreferences) {
    const clean = normalize(input);
    const next = resolved(clean);
    const current = resolved(preferences);
    const errors = (Object.entries(clean) as Array<[keyof LocationPreferences, string | null]>).flatMap(([key, path]) => path && !/^(?:[a-z]:[\\\\/]|\\\\\\\\[^\\\\]+[\\\\][^\\\\]+)/i.test(path) ? [labels[key] + '需要填写完整的绝对路径。'] : []);
    const changes = (Object.keys(labels) as Array<keyof LocationPreferences>).filter((key) => next[key] !== current[key]).map((key) => ({
      key, label: labels[key], currentPath: current[key], nextPath: next[key], migration: key === 'codexHome' || key === 'sqliteHome' ? 'switch' as const : 'copy' as const, files: 0, bytes: 0,
    }));
    return {
      expectedHash: JSON.stringify([revision, clean]), preferences: clean, resolved: next, changes,
      warnings: changes.some((change) => change.migration === 'switch') ? ['官方 Codex 数据目录只切换位置，不自动迁移原始数据库。'] : [],
      errors, canSave: errors.length === 0, requiresRestart: changes.length > 0,
    };
  }
  return {
    async status() { if (failStatus) { failStatus = false; throw new Error('位置设置暂时不可读，请重新读取。'); } return status(); },
    async preview(input) { return preview(input); },
    async save(input, expectedHash) {
      const checked = preview(input);
      if (checked.expectedHash !== expectedHash) throw new Error('位置状态已经变化，请重新检查更改。');
      if (!checked.canSave) throw new Error(checked.errors.join(' '));
      pendingPreferences = JSON.stringify(checked.preferences) === JSON.stringify(preferences) ? null : structuredClone(checked.preferences);
      revision += 1;
      return status();
    },
  };
}
