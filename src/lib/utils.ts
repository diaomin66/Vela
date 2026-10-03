import type { ProfileInput } from '../types';

export function endpointHost(value: string): string {
  try { return new URL(value).host; } catch { return value; }
}

export function validateProfileInput(input: ProfileInput, existing = false): string | null {
  if (!input.name.trim()) return '给这个连接起一个名字吧。';
  if (input.name.trim().length > 80) return '连接名称请控制在 80 个字符以内。';
  let url: URL;
  try { url = new URL(input.baseUrl.trim()); } catch { return '请输入完整的 API 地址，例如 https://api.example.com/v1。'; }
  const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
  if (url.protocol !== 'https:' && !(url.protocol === 'http:' && local)) return 'API 地址需要使用 HTTPS；本机服务可以使用 HTTP。';
  if (url.username || url.password || url.search || url.hash) return 'API 地址不能包含账号、密码、查询参数或片段。';
  if (!input.model.trim() && !input.models) return '请输入服务商提供的模型名称。';
  if (!existing && !input.apiKey?.trim()) return '请输入这个服务商的 API Key。';
  return null;
}

export function readableTime(value?: string | null): string {
  if (!value) return '尚未验证';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '时间未知';
  return new Intl.DateTimeFormat('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false }).format(date);
}

export function errorMessage(error: unknown): string {
  const value = error instanceof Error ? error.message : typeof error === 'string' ? error : '操作未完成，请重试。';
  return value.replace(/\bsk-[A-Za-z0-9_-]{4,}/g, '[已隐藏密钥]').replace(/Bearer\s+[^\s"',;]+/gi, 'Bearer [已隐藏密钥]');
}

export function downloadReport(report: unknown) {
  const payload = JSON.stringify(report, (key, value: unknown) => {
    if (/api.?key|token|secret|authorization/i.test(key)) return '[已隐藏]';
    return typeof value === 'string' ? errorMessage(value) : value;
  }, 2);
  const href = URL.createObjectURL(new Blob([payload], { type: 'application/json;charset=utf-8' }));
  const link = document.createElement('a');
  link.href = href;
  link.download = `vela-diagnostics-${new Date().toISOString().slice(0, 10)}.json`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(href), 1000);
}
