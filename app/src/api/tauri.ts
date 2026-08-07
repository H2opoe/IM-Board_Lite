export const isTauri = "__TAURI_INTERNALS__" in window;

export function requireTauri<T>(action: string): Promise<T> {
  return Promise.reject(new Error(`${action}需要在IM-Board桌面应用中运行，请使用Tauri启动。`));
}
