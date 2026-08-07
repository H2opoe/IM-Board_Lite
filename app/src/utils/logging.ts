export function logRecoverableError(scope: string, error: unknown): void {
  // 开发者日志保留原始错误对象，用户可见提示由调用处用中文兜底文案承接。
  console.warn(`[IM-Board] ${scope}`, error);
}
