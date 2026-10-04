import type { AppApi } from './api';
import type { Backup, ChangePreview, ChannelModel, Dashboard, DiagnosticItem, Profile } from '../types';

const delay = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
const now = () => new Date().toISOString();
const item = (id: string, title: string, description: string): DiagnosticItem => ({ id, category: 'preview', title, description, status: 'passed', repairable: false });
const effortOrder = ['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'];
// Demo capabilities are explicit fixtures. Native capability policy lives in reasoning.rs.
const demoReasoning = (model: ChannelModel) => {
  const efforts = effortOrder.filter((effort) => model.reasoningEfforts?.includes(effort));
  const preferred = model.defaultReasoningEffort;
  const nativeEffort = ['max', 'xhigh', 'high', 'medium', 'low'].find((effort) => efforts.includes(effort));
  const nativeReasoning = efforts.includes('ultra') && nativeEffort
    ? model.nativeReasoning && efforts.includes(model.nativeReasoning.ultraEffort) ? model.nativeReasoning : { multiAgentVersion: 'v2', ultraEffort: nativeEffort }
    : null;
  return { supportedReasoningEfforts: efforts, apiReasoningEfforts: efforts.filter((effort) => effort !== 'ultra'), defaultReasoningEffort: preferred && efforts.includes(preferred) ? preferred : efforts.includes('medium') ? 'medium' : efforts[0] ?? null, nativeReasoning };
};

export function createPreviewApi(): AppApi {
  const state: Dashboard = {
    environment: { platform: 'Windows · 演示', codexInstalled: false, configPath: '%USERPROFILE%\\.codex\\config.toml', configExists: false, configValid: true, appVersion: '0.11.0', desktopMode: false },
    profiles: [
      { id: 'demo-work', name: '主力渠道', baseUrl: 'https://api.example.com', resolvedBaseUrl: 'https://api.example.com/v1', model: 'example-code', models: [{ id: 'example-code', alias: '编程主力', enabled: true }, { id: 'example-pro', alias: '', enabled: true }, { id: 'example-fast', alias: '', enabled: false }], balanceConfig: { mode: 'auto' }, balance: { status: 'available', remaining: 128.50, unit: '站点计费单位', source: '示例数据', checkedAt: now() }, keyStored: true, createdAt: now(), updatedAt: now(), lastSyncedAt: now(), revision: crypto.randomUUID() },
      { id: 'demo-lab', name: '备用渠道', baseUrl: 'https://gateway.example.com/v1', model: 'example-code', models: [{ id: 'example-code', alias: '', enabled: true }, { id: 'example-reasoning', alias: '', enabled: true }], balanceConfig: { mode: 'auto' }, balance: { status: 'unsupported', remaining: null, unit: '额度', source: '示例数据', checkedAt: now(), message: '该服务商不提供可识别的余额接口。' }, keyStored: true, createdAt: now(), updatedAt: now(), lastSyncedAt: now(), revision: crypto.randomUUID() },
    ],
    settings: { providerName: 'Vela', gatewayPort: 18761, autoRefresh: true, refreshMinutes: 15 },
    gateway: { running: true, port: 18761, baseUrl: 'http://127.0.0.1:18761/v1' },
    catalog: [],
    gatewayApplied: true,
    gatewayConfigured: true,
    defaultRouteId: 'demo-work/example-code',
    activeProfileId: 'demo-work',
    activeProfileMatches: true,
    backups: [],
  };
  for (const p of state.profiles) for (const m of p.models) {
    if (m.id !== 'example-fast') { m.reasoningEfforts = ['low', 'medium', 'high', 'xhigh']; m.defaultReasoningEffort = 'medium'; }
  }
  let appliedModels = structuredClone(state.profiles.map(({ id, models }) => ({ id, models })));
  const updateCatalog = () => {
    state.catalog = state.profiles.flatMap((p) => p.models.map((m) => ({ routeId: `${p.id}/${m.id}`, profileId: p.id, channelName: p.name, modelId: m.id, displayName: `${p.name}${m.alias ? ' · ' + m.alias : ''}（${m.id}）`, enabled: m.enabled, ...demoReasoning(m) })));
  };
  updateCatalog();
  const cancelled = new Set<string>();
  const snapshots = new Map<string, Pick<Dashboard, 'activeProfileId' | 'defaultRouteId' | 'gatewayApplied' | 'gatewayConfigured' | 'settings' | 'gateway'> & { models: typeof appliedModels }>();
  let revision = 0;
  const profile = (id: string) => {
    const found = state.profiles.find((p) => p.id === id);
    if (!found) throw new Error('这个连接已经不存在，请刷新后重试。');
    return found;
  };
  const backup = (reason: string): Backup => {
    const record = { id: crypto.randomUUID(), createdAt: now(), reason, summary: '演示备份，仅存在于当前页面内存。', configExisted: true };
    snapshots.set(record.id, structuredClone({ activeProfileId: state.activeProfileId, defaultRouteId: state.defaultRouteId, gatewayApplied: state.gatewayConfigured, gatewayConfigured: state.gatewayConfigured, settings: state.settings, gateway: state.gateway, models: appliedModels }));
    state.backups.unshift(record);
    revision++;
    return record;
  };
  const preview = (title: string, before: string, after: string): ChangePreview => ({ id: crypto.randomUUID(), title, summary: '浏览器演示：不读取或修改本机 Codex。', changes: [{ label: '当前连接', before, after }], expectedHash: String(revision) });
  const check = (hash: string) => { if (hash !== String(revision)) throw new Error('配置已经发生变化，请重新预览。'); };
  return {
    dashboard: async () => { updateCatalog(); return structuredClone(state); },
    saveProfile: async (input) => {
      await delay(250);
      const original = input.id ? profile(input.id) : undefined;
      if (original && input.baseUrl.trim().replace(/\/$/, '') !== original.baseUrl) throw new Error('已保存渠道的 API 地址不能修改，请新建渠道。');
      const result: Profile = { ...original, models: input.models ?? original?.models ?? [], balanceConfig: input.balanceConfig ?? original?.balanceConfig ?? { mode: 'auto' }, id: input.id ?? crypto.randomUUID(), name: input.name.trim(), baseUrl: input.baseUrl.trim().replace(/\/$/, ''), model: input.model.trim(), keyStored: !!input.apiKey?.trim() || !!original?.keyStored, createdAt: original?.createdAt ?? now(), updatedAt: now(), revision: crypto.randomUUID() };
      result.models = result.models.map((model) => {
        const reasoning = demoReasoning(model);
        if (model.reasoningEfforts?.includes('ultra') && !reasoning.nativeReasoning) throw new Error(`${model.id} 的 Ultra 需要至少一个低及以上的常规推理档位。`);
        return { ...model, nativeReasoning: reasoning.nativeReasoning };
      });
      const index = state.profiles.findIndex((p) => p.id === result.id);
      if (index < 0) state.profiles.push(result); else state.profiles[index] = result;
      state.gatewayApplied = false; revision++;
      if (result.id === state.activeProfileId) state.activeProfileMatches = false;
      updateCatalog();
      return result;
    },
    deleteProfile: async (id) => {
      if (id === state.activeProfileId) throw new Error('请先切换到其他连接，或恢复接入前的配置。');
      state.profiles = state.profiles.filter((p) => p.id !== id);
      state.gatewayApplied = false; revision++;
    },
    syncProfile: async (id, runId) => {
      await delay(650);
      if (runId && cancelled.delete(runId)) throw new Error('同步已取消。');
      const p = profile(id);
      const discovered = ['example-code', 'example-pro', 'example-fast'];
      p.models = [...p.models, ...discovered.filter((model) => !p.models.some((m) => m.id === model)).map((model) => ({ id: model, alias: '', enabled: false }))];
      p.resolvedBaseUrl = p.baseUrl.replace(/\/(responses|models)\/?$/, '').replace(/\/$/, '');
      if (!p.resolvedBaseUrl.endsWith('/v1')) p.resolvedBaseUrl += '/v1';
      p.lastSyncedAt = now(); p.syncError = null;
      p.balance = p.balanceConfig.mode === 'disabled'
        ? { status: 'disabled', remaining: null, unit: '额度', source: '演示', checkedAt: now() }
        : { status: 'available', remaining: 128.5, unit: p.balanceConfig.unit || '站点计费单位', source: '示例数据', checkedAt: now() };
      revision++; updateCatalog(); return structuredClone(p);
    },
    saveSettings: async (input) => {
      if (!input.providerName.trim()) throw new Error('请输入服务商显示名称。');
      if (input.gatewayPort < 1024 || input.gatewayPort > 65535) throw new Error('端口范围为 1024–65535。');
      state.settings = { ...input, providerName: input.providerName.trim() };
      state.gatewayApplied = false;
      revision++;
      return structuredClone(state.settings);
    },
    previewGateway: async (routeId) => {
      updateCatalog();
      const selected = state.catalog.find((m) => m.routeId === (routeId ?? state.defaultRouteId) && m.enabled) ?? state.catalog.find((m) => m.enabled);
      if (!selected) throw new Error('请先在渠道中启用至少一个模型。');
      return { ...preview('接入统一模型库', state.gatewayApplied ? 'AhaX 已接入' : '原有配置', state.settings.providerName), changes: [{ label: '服务商显示名称', before: '当前服务商', after: state.settings.providerName }, { label: '默认模型', before: state.catalog.find((m) => m.routeId === state.defaultRouteId)?.displayName ?? '未设置', after: selected.displayName }, { label: '统一模型库', before: '', after: `${state.catalog.filter((m) => m.enabled).length} 个模型 · ${state.profiles.length} 个渠道` }] };
    },
    applyGateway: async (routeId, hash) => {
      check(hash); updateCatalog();
      const selected = state.catalog.find((m) => m.routeId === (routeId ?? state.defaultRouteId) && m.enabled) ?? state.catalog.find((m) => m.enabled);
      if (!selected) throw new Error('请启用至少一个模型。');
      const record = backup('接入统一模型库'); state.gatewayApplied = true; state.gatewayConfigured = true; state.defaultRouteId = selected.routeId; state.activeProfileId = selected.profileId; state.activeProfileMatches = true; state.gateway = { running: true, port: state.settings.gatewayPort, baseUrl: `http://127.0.0.1:${state.settings.gatewayPort}/v1` }; appliedModels = structuredClone(state.profiles.map(({ id, models }) => ({ id, models }))); return record;
    },
    validateProfile: async (id, runId) => {
      await delay(1000);
      if (cancelled.delete(runId)) throw new Error('验证已取消。');
      profile(id).lastValidatedAt = now();
      return { ok: true, checkedAt: now(), latencyMs: 186, capabilities: { responses: true, streaming: true, tools: true }, items: [item('responses', 'Responses 请求', '演示结果：桌面版会发送真实请求验证。'), item('streaming', '流式输出', '演示结果：检查服务端事件流是否完整。'), item('tools', '工具调用与结果回传', '演示结果：验证多步任务所需的工具回传。')] };
    },
    previewProfile: async (id) => ({ ...preview('应用连接', state.profiles.find((p) => p.id === state.activeProfileId)?.name ?? '原有配置', profile(id).name), profileId: id }),
    applyProfile: async (id, hash) => { check(hash); profile(id); const record = backup('切换连接'); state.activeProfileId = id; state.activeProfileMatches = true; return record; },
    diagnose: async (runId) => {
      await delay(950);
      if (cancelled.delete(runId)) throw new Error('检查已取消。');
      return { id: crypto.randomUUID(), createdAt: now(), summary: '演示检查完成，以下为示例结果。', canRepair: false, items: [item('config', '配置文件结构', '演示：文件语法与必要字段检查通过。'), item('provider', '服务商引用', '演示：当前模型与服务商配置对应。'), item('credential', '密钥存储', '演示：桌面版通过 Windows 凭据管理器读取。'), { id: 'runtime', category: 'runtime', title: '需要在桌面版验证实际环境', status: 'info', description: '浏览器预览无法读取本机配置，也不会发起 API 请求。', repairable: false }] };
    },
    cancel: async (runId) => { cancelled.add(runId); },
    previewRepair: async () => preview('修复当前连接', '待修复配置', '连接的已保存配置'),
    repair: async (hash) => { check(hash); return backup('修复配置'); },
    previewRestore: async (id) => {
      if (!snapshots.has(id)) throw new Error('备份不存在。');
      return { ...preview('恢复配置', '当前配置', '所选备份中的配置'), backupId: id };
    },
    restore: async (id, hash) => {
      check(hash); const snapshot = snapshots.get(id); if (!snapshot) throw new Error('备份不存在。');
      const record = backup('恢复备份前'); const { models, ...restored } = structuredClone(snapshot); Object.assign(state, restored);
      for (const p of state.profiles) { const saved = models.find((item) => item.id === p.id); if (saved) p.models = saved.models; }
      appliedModels = models; updateCatalog(); return record;
    },
    openCodex: async () => '当前是浏览器预览。请在桌面应用中打开 Codex。',
  };
}
