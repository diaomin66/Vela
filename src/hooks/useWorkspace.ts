import { useCallback, useEffect, useRef, useState } from 'react';
import { api, desktop } from '../lib/api';
import { errorMessage } from '../lib/utils';
import type { Backup, CatalogEntry, ChangePreview, Dashboard, DiagnosticReport, Profile, ValidationResult, View } from '../types';

export type SettingsPage = 'general' | 'locations' | 'updates';

export type WorkspaceModal =
  | { type: 'editor'; profile?: Profile; initialModelId?: string }
  | { type: 'validate'; profile: Profile; modelId?: string }
  | { type: 'change'; preview: ChangePreview; operation: 'restore' | 'repair' | 'gateway'; targetId?: string }
  | { type: 'delete'; profile: Profile }
  | { type: 'settings'; initialPage?: SettingsPage }
  | null;

export function useWorkspace() {
  const [view, setView] = useState<View>('connections');
  const [data, setData] = useState<Dashboard | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [modal, setModal] = useState<WorkspaceModal>(null);
  const [toast, setToast] = useState<{ message: string; error?: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const [diagnosing, setDiagnosing] = useState(false);
  const [report, setReport] = useState<DiagnosticReport | null>(null);
  const [validation, setValidation] = useState<ValidationResult | null>(null);
  const [validating, setValidating] = useState(false);
  const [modalError, setModalError] = useState<string | null>(null);
  const [syncingIds, setSyncingIds] = useState<Set<string>>(new Set());
  const validationRun = useRef<string | null>(null);
  const diagnosisRun = useRef<string | null>(null);
  const synchronizationRuns = useRef(new Map<string, string>());
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const refreshSequence = useRef(0);
  const pendingReads = useRef(0);
  const alive = useRef(true);
  const reasoningSave = useRef(false);

  const notify = useCallback((message: string, error = false) => {
    setToast({ message, error });
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setToast(null), error ? 8000 : 4500);
  }, []);
  const refresh = useCallback(async () => {
    const sequence = ++refreshSequence.current; pendingReads.current++;
    try {
      const next = await api.dashboard();
      if (alive.current && sequence === refreshSequence.current) { setData(next); setLoadError(null); }
      return next;
    } finally { pendingReads.current--; }
  }, []);
  useEffect(() => {
    alive.current = true;
    void refresh().catch((err: unknown) => { if (alive.current) setLoadError(errorMessage(err)); });
    const poll = () => { if (!document.hidden && !pendingReads.current) void refresh().catch(() => {}); };
    const interval = setInterval(poll, 15000);
    window.addEventListener('focus', poll);
    return () => {
      alive.current = false; clearInterval(interval); window.removeEventListener('focus', poll);
      if (timer.current) clearTimeout(timer.current);
      for (const run of [validationRun.current, diagnosisRun.current, ...synchronizationRuns.current.values()]) if (run) void api.cancel(run).catch(() => {});
    };
  }, [refresh]);

  function openEditor(profile?: Profile, initialModelId?: string) { if (reasoningSave.current) return; setModalError(null); setModal({ type: 'editor', profile, initialModelId }); }
  function openValidation(profile: Profile, modelId?: string) { setValidation(null); setModalError(null); setModal({ type: 'validate', profile, modelId }); }
  function closeModal() {
    if (busy) return;
    const run = validationRun.current; validationRun.current = null;
    if (run) void api.cancel(run).catch(() => {});
    setValidating(false); setModal(null); setModalError(null); setValidation(null);
  }
  async function validate() {
    if (modal?.type !== 'validate') return;
    const run = crypto.randomUUID(); validationRun.current = run;
    setValidating(true); setValidation(null); setModalError(null);
    try {
      const result = await api.validateProfile(modal.profile.id, run, modal.modelId);
      if (validationRun.current !== run) return;
      setValidation(result); await refresh();
    } catch (err) { if (validationRun.current === run) setModalError(errorMessage(err)); }
    finally { if (validationRun.current === run) { validationRun.current = null; setValidating(false); } }
  }
  async function preview(operation: 'gateway' | 'repair' | 'restore', targetId?: string) {
    setBusy(true); setModalError(null);
    try {
      const result = operation === 'gateway' ? await api.previewGateway(targetId) : operation === 'restore' ? await api.previewRestore(targetId!) : await api.previewRepair();
      setModal({ type: 'change', preview: result, operation, targetId });
    } catch (err) { notify(errorMessage(err), true); }
    finally { setBusy(false); }
  }
  async function applyChange() {
    if (modal?.type !== 'change') return;
    const operation = modal.operation;
    setBusy(true); setModalError(null);
    try {
      if (modal.operation === 'gateway') await api.applyGateway(modal.targetId, modal.preview.expectedHash);
      else if (modal.operation === 'restore') await api.restore(modal.targetId!, modal.preview.expectedHash);
      else await api.repair(modal.preview.expectedHash);
      await refresh(); setModal(null); setReport(null);
      notify(desktop ? '已备份并更新配置。请彻底退出并重开 Codex，再新建会话；旧会话可能保留原服务商。' : '演示配置已更新，本机配置未改变。');
      if (operation === 'repair') await diagnose();
    } catch (err) { setModalError(errorMessage(err)); }
    finally { setBusy(false); }
  }
  async function syncChannel(profile: Profile) {
    if (synchronizationRuns.current.has(profile.id)) return;
    const run = crypto.randomUUID(); synchronizationRuns.current.set(profile.id, run);
    setSyncingIds((ids) => new Set(ids).add(profile.id));
    try { const result = await api.syncProfile(profile.id, run); await refresh(); notify(result.syncError || `${profile.name}已更新`, !!result.syncError); }
    catch (err) { notify(errorMessage(err), true); }
    finally {
      synchronizationRuns.current.delete(profile.id);
      setSyncingIds((ids) => { const next = new Set(ids); next.delete(profile.id); return next; });
    }
  }
  async function diagnose() {
    const run = crypto.randomUUID(); diagnosisRun.current = run;
    setDiagnosing(true); setReport(null);
    try { const result = await api.diagnose(run); if (diagnosisRun.current === run) { setReport(result); await refresh(); } }
    catch (err) { if (diagnosisRun.current === run) notify(errorMessage(err), true); }
    finally { if (diagnosisRun.current === run) { diagnosisRun.current = null; setDiagnosing(false); } }
  }
  function cancelDiagnosis() {
    const run = diagnosisRun.current; diagnosisRun.current = null; setDiagnosing(false);
    if (run) void api.cancel(run).catch(() => {});
  }
  function navigate(next: View) { setView(next); if (next !== 'diagnostics') cancelDiagnosis(); }
  async function openCodex() { try { notify(await api.openCodex()); } catch (err) { notify(errorMessage(err), true); } }
  async function deleteProfile() {
    if (modal?.type !== 'delete') return;
    setBusy(true); setModalError(null);
    try { await api.deleteProfile(modal.profile.id); await refresh(); setModal(null); notify('渠道已删除'); }
    catch (err) { setModalError(errorMessage(err)); }
    finally { setBusy(false); }
  }
  async function setModelReasoning(entry: CatalogEntry, effort: string) {
    if (reasoningSave.current || busy || effort === entry.defaultReasoningEffort || !entry.supportedReasoningEfforts.includes(effort)) return;
    reasoningSave.current = true; setBusy(true);
    try {
      const current = await api.dashboard();
      const profile = current.profiles.find((item) => item.id === entry.profileId);
      if (!profile) throw new Error('渠道已不存在');
      await api.saveProfile({ id: profile.id, name: profile.name, baseUrl: profile.baseUrl, model: profile.model, balanceConfig: profile.balanceConfig, models: profile.models.map((model) => model.id === entry.modelId ? { ...model, defaultReasoningEffort: effort } : model) });
      await refresh();
      notify('推理强度已保存，更新 Codex 配置后生效。');
    } catch (err) { notify(errorMessage(err), true); }
    finally { reasoningSave.current = false; setBusy(false); }
  }
  return {
    view, data, loadError, modal, toast, busy, diagnosing, report, validation, validating, modalError, syncingIds,
    refresh, notify, navigate, setModal, setToast, openEditor, openValidation, closeModal, validate, applyChange,
    syncChannel, diagnose, cancelDiagnosis, openCodex, deleteProfile, setModelReasoning,
    prepareGateway: (routeId?: string) => preview('gateway', routeId),
    prepareRestore: (backup: Backup) => preview('restore', backup.id),
    prepareRepair: () => preview('repair'),
  };
}
export type WorkspaceController = ReturnType<typeof useWorkspace>;
