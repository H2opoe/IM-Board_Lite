export function userErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  if (typeof error === "string" && error.trim()) return error;

  // 外部 CLI / AI SDK 报错要保留原文；只有无法提取可读文本时才使用应用自己的中文兜底。
  return fallback;
}
