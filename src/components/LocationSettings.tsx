import { Check, FolderCog, FolderOpen, LoaderCircle, RotateCcw } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { desktop } from '../lib/api';
import { locationsApi } from '../lib/locations/api';
import { DEFAULT_LOCATION_PREFERENCES, type LocationPreferences, type LocationPreview, type LocationStatus } from '../lib/locations/types';
import { errorMessage } from '../lib/utils';

const fields: Array<[keyof LocationPreferences, string, string]> = [
  ['codexHome', 'Codex 数据目录', '官方线程与 config.toml 所在目录'],
  ['sqliteHome', '索引数据库目录', '留空时遵循 Codex 的 sqlite_home 配置'],
  ['backupsDirectory', '配置备份目录', '保存 ahaX 配置快照'],
  ['evaluationsDirectory', '评测数据目录', '保存评测结果与动图产物'],
  ['exportsDirectory', '导出目录', '留空时跟随评测数据目录'],
  ['threadProtectionDirectory', '线程保护目录', '保存加密线程副本和回收记录'],
  ['threadIndexDirectory', '线程索引目录', '留空时跟随线程保护目录'],
];
const fieldGroups = [
  { id: 'official', title: 'Codex 数据', description: '切换访问位置，不搬迁官方配置、会话或数据库。', fields: fields.slice(0, 2) },
  { id: 'application', title: 'ahaX 数据', description: '数据会校验后复制，原目录的文件保留。', fields: fields.slice(2, 5) },
  { id: 'threads', title: '线程保护', description: '保护副本与检索索引可以分别存放。', fields: fields.slice(5) },
];
const locationKey = ['locations', 'status'] as const;
const normalized = (value: LocationPreferences): LocationPreferences => Object.fromEntries(Object.entries(value).map(([key, path]) => [key, path?.trim() || null])) as unknown as LocationPreferences;

export function LocationSettings({ onBusyChange, onDirtyChange }: { onBusyChange?: (busy: boolean) => void; onDirtyChange?: (dirty: boolean) => void }) {
  const query = useQuery({ queryKey: locationKey, queryFn: locationsApi.status, retry: false, staleTime: 0 });
  if (!query.data) return <section className="location-settings editor-section" aria-label="数据位置">
    {query.error ? <div className="location-load-error"><strong>数据位置暂时无法读取</strong><p className="inline-error" role="alert">{errorMessage(query.error)}</p><button type="button" className="button button-quiet" disabled={query.isFetching} onClick={() => void query.refetch()}>{query.isFetching && <LoaderCircle className="spin" size={14}/>}重新读取位置</button></div> : <div className="location-loading" role="status"><LoaderCircle className="spin" size={17}/>正在读取位置设置</div>}
  </section>;
  return <LocationEditor status={query.data} onBusyChange={onBusyChange} onDirtyChange={onDirtyChange}/>;
}

function LocationEditor({ status, onBusyChange, onDirtyChange }: { status: LocationStatus; onBusyChange?: (busy: boolean) => void; onDirtyChange?: (dirty: boolean) => void }) {
  const client = useQueryClient();
  const [draft, setDraft] = useState<LocationPreferences>(() => status.pendingPreferences || status.preferences);
  const [preview, setPreview] = useState<LocationPreview | null>(null);
  const [operation, setOperation] = useState<'preview' | 'save' | 'browse' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const feedback = useRef<HTMLDivElement>(null);
  const busy = operation !== null;
  useEffect(() => { onBusyChange?.(busy); return () => onBusyChange?.(false); }, [busy, onBusyChange]);
  const changed = JSON.stringify(normalized(draft)) !== JSON.stringify(status.pendingPreferences || status.preferences);
  useEffect(() => { onDirtyChange?.(changed); return () => onDirtyChange?.(false); }, [changed, onDirtyChange]);
  useEffect(() => { if (preview || error) feedback.current?.scrollIntoView({ block: 'nearest' }); }, [preview, error]);
  const hasCustomPaths = Object.values(draft).some((path) => Boolean(path?.trim()));
  function edit(next: LocationPreferences) { setDraft(next); setPreview(null); setError(null); }
  async function browse(key: keyof LocationPreferences) {
    if (busy) return;
    setOperation('browse'); setError(null);
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const path = await open({ directory: true, multiple: false, defaultPath: draft[key] || status.active[key] });
      if (typeof path === 'string') edit({ ...draft, [key]: path });
    } catch (value) { setError(errorMessage(value)); }
    finally { setOperation(null); }
  }
  async function inspect(input = normalized(draft)) {
    if (busy) return;
    setOperation('preview'); setError(null); setPreview(null);
    try { setPreview(await locationsApi.preview(input)); }
    catch (value) { setError(errorMessage(value)); }
    finally { setOperation(null); }
  }
  async function save() {
    if (!preview?.canSave || busy) return;
    setOperation('save'); setError(null);
    try {
      const next = await locationsApi.save(preview.preferences, preview.expectedHash);
      client.setQueryData(locationKey, next);
      setDraft(next.pendingPreferences || next.preferences); setPreview(null);
    } catch (value) { setError(errorMessage(value)); setPreview(null); }
    finally { setOperation(null); }
  }
  return <section className="location-settings location-page" aria-label="数据位置">
    <div className="location-fields location-page-scroll" id="location-fields">
      <p className="editor-hint location-introduction">填写完整目录路径，留空使用默认位置。更改将在 ahaX 完全退出并重新启动后生效。</p>
      {status.error && <p className="inline-error" role="alert">{status.error}</p>}
      {fieldGroups.map((group) => <section className="location-group" key={group.id} aria-labelledby={`location-group-${group.id}`}>
        <div className="location-group-heading"><h3 id={`location-group-${group.id}`}>{group.title}</h3><p>{group.description}</p></div>
        {group.fields.map(([key, label, hint]) => {
        const override = status.overrides.find((item) => item.key === key);
        return <div className="editor-field location-field" key={key}>
          <label htmlFor={`location-${key}`}>{label}</label>
          <div className="location-input"><input id={`location-${key}`} value={draft[key] || ''} placeholder="留空使用默认位置" disabled={busy || Boolean(override)} spellCheck={false} autoComplete="off" onChange={(event) => edit({ ...draft, [key]: event.target.value || null })} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); if (changed) void inspect(); } }}/>{desktop && <button type="button" className="icon-button" aria-label={`浏览${label}`} disabled={busy || Boolean(override)} onClick={() => void browse(key)}><FolderOpen size={17}/></button>}</div>
          <small>{override ? `由环境变量 ${override.environment} 指定；需在启动环境中修改。` : hint}</small>
          <div className="location-current"><span>当前</span><code>{status.active[key]}</code></div>
          {status.next[key] !== status.active[key] && <div className="location-next"><span>下次启动</span><code>{status.next[key]}</code></div>}
        </div>;
        })}
      </section>)}
      <div ref={feedback} className="location-feedback">{preview && <div className="location-preview" role="status"><strong>{preview.errors.length ? '有位置需要调整' : preview.changes.length ? `已检查 ${preview.changes.length} 项位置更改` : '实际位置未发生变化'}</strong>{preview.changes.map((change) => <div key={change.key}><span>{change.label}</span><code>{change.currentPath} → {change.nextPath}</code><small>{change.migration === 'switch' ? '切换到已有目录' : '保留原文件并复制到新目录'} · {change.files} 个文件</small></div>)}{preview.warnings.map((warning) => <p className="location-warning" key={warning}>{warning}</p>)}{preview.errors.map((message) => <p className="inline-error" key={message}>{message}</p>)}</div>}
      {error && <p className="inline-error" role="alert">{error}</p>}</div>
      {status.requiresRestart && <div className="location-restart" role="status"><p>已保存，下次启动 ahaX 时应用。当前会话继续使用上方「当前」位置。</p><button type="button" className="text-button" disabled={busy} onClick={() => { edit(status.preferences); void inspect(status.preferences); }}>预览撤回更改</button></div>}
    </div>
    <footer className="editor-footer"><div className="location-actions"><button type="button" className="button button-quiet" disabled={busy || !changed && !hasCustomPaths} onClick={() => edit(changed ? status.pendingPreferences || status.preferences : { ...DEFAULT_LOCATION_PREFERENCES })}><RotateCcw size={14}/>{changed ? '撤销编辑' : '恢复默认'}</button>{preview?.canSave ? <button type="button" className="button button-primary" disabled={busy} onClick={() => void save()}>{operation === 'save' ? <LoaderCircle className="spin" size={15}/> : <Check size={15}/>}保存位置更改</button> : <button type="button" className="button button-primary" disabled={busy || !changed} onClick={() => void inspect()}>{operation === 'preview' ? <LoaderCircle className="spin" size={15}/> : <FolderCog size={15}/>}检查位置更改</button>}</div></footer>
  </section>;
}
