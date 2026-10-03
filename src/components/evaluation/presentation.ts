import type { EvaluationActivity, EvaluationCaseId } from '../../lib/evaluation';
import { effortLabels } from '../../lib/models';

export type EvaluationRecord = EvaluationActivity['records'][number];
export const caseTitles: Record<EvaluationCaseId, string> = { pelican: '鹈鹕动画', candy: '糖果推理', judgment: '模型判题' };
export const resultLabels = { passed: '通过', generated: '已生成', failed: '未通过', error: '请求失败', cancelled: '已取消' };
export const runLabels = { running: '进行中', completed: '已完成', cancelled: '已取消', interrupted: '已中断' };
export const effortName = (effort: string | null) => effortLabels[effort ?? ''] ?? '默认';
export const duration = (milliseconds: number) => `${(milliseconds / 1000).toFixed(1)} 秒`;
export const recordTime = (value: string) => new Intl.DateTimeFormat('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false }).format(new Date(value));
export const intervalLabel = (minutes: number) => minutes === 1440 ? '每天' : minutes < 60 ? `每 ${minutes} 分钟` : minutes % 60 === 0 ? `每 ${minutes / 60} 小时` : `每 ${minutes} 分钟`;
export const recordKey = (record: Pick<EvaluationRecord, 'profileId' | 'modelId' | 'reasoningEffort'>) => JSON.stringify([record.profileId, record.modelId, record.reasoningEffort]);
