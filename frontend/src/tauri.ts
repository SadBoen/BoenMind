// Tauri 桌面环境适配。
// 浏览器/纯 web-server 下不加载（无 __TAURI_INTERNALS__），各工具优雅降级。

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
    __TAURI__?: { core: { invoke: (cmd: string, args?: unknown) => Promise<unknown> } };
  }
}

export const isTauri = (): boolean =>
  typeof window !== "undefined" && !!window.__TAURI_INTERNALS__;

/** 调 Tauri command；非 Tauri 环境返回 null（不抛错）。 */
export async function invokeTauri<T>(cmd: string, args?: unknown): Promise<T | null> {
  if (!isTauri() || !window.__TAURI__?.core?.invoke) return null;
  try {
    return (await window.__TAURI__.core.invoke(cmd, args)) as T;
  } catch (e) {
    console.warn(`[tauri] ${cmd} failed:`, e);
    return null;
  }
}

export interface UpdateCheckResult {
  available: boolean;
  version?: string;
  date?: string;
  body?: string;
}