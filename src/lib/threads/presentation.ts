import type { ThreadSummary } from './types';

export const threadTitle = (thread: ThreadSummary) => thread.title?.trim() || '未命名线程';
export const threadDependencyIssue = (thread: ThreadSummary): 'missing' | 'cycle' | null => thread.integrity === 'dependency-cycle' ? 'cycle' : thread.integrity === 'dependency-missing' || thread.recoverability === 'dependencies-missing' ? 'missing' : null;
export const threadNeedsAttention = (thread: ThreadSummary) => threadDependencyIssue(thread) !== null || thread.recoverability === 'recoverable' || thread.integrity !== 'valid' || thread.stateIndex === 'missing' || !['protected', 'notNeeded'].includes(thread.snapshot);
export const canRestoreThread = (thread: ThreadSummary) => thread.recoverability === 'recoverable' && threadDependencyIssue(thread) === null;
export const shortFolder = (path: string | null) => path?.replace(/[\\/]+$/, '').split(/[\\/]/).at(-1) || '未记录工作目录';

export function threadStatus(thread: ThreadSummary): { label: string; tone: 'neutral' | 'success' | 'warning' } {
  const dependency = threadDependencyIssue(thread);
  if (dependency) return { label: dependency === 'cycle' ? '历史引用循环' : '历史基础缺失', tone: 'warning' };
  if (thread.recoverability === 'recoverable') return { label: '可找回', tone: 'warning' };
  if (thread.integrity === 'missing') return { label: '原文件缺失', tone: 'warning' };
  if (thread.integrity === 'changed') return { label: '等待稳定', tone: 'neutral' };
  if (thread.integrity !== 'valid') return { label: '需要检查', tone: 'warning' };
  if (thread.stateIndex === 'missing') return { label: '列表索引缺失', tone: 'warning' };
  if (thread.snapshot === 'protected') return { label: '已有备份', tone: 'success' };
  if (thread.snapshot === 'pending') return { label: '待备份', tone: 'neutral' };
  if (thread.snapshot === 'notNeeded') return { label: '未纳入保护', tone: 'neutral' };
  return { label: '尚未备份', tone: 'warning' };
}

export function integrityLabel(value: string) {
  return ({ valid: '记录格式完整', partial: '部分记录未完成或无法解析', corrupt: '部分完整记录已损坏', unrecognized: '格式尚未识别', unreadable: '暂时无法读取', changed: '文件正在变化', missing: '原文件已缺失', 'source-unavailable': '数据来源暂时不可用', 'dependency-missing': '历史基础记录缺失', 'dependency-cycle': '历史引用存在循环', 'too-large': '超出本次读取范围' } as Record<string, string>)[value] ?? '需要进一步检查';
}

export function formatThreadBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1048576) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1048576).toFixed(1)} MB`;
}

export function threadTime(value: string | null) {
  if (!value) return '尚未记录';
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? '时间未知' : new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false }).format(date);
}
