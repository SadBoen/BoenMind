// 统一前端 localStorage 键常量与安全存取器(消除散落的魔法字符串与拼写错误)

export const STORAGE_KEYS = {
  ACTIVE_MODEL: "bm_active_model",
  ACTIVE_ROLE: "bm_active_role",
  SESSION: "bm_session",
  PINS: "bm_pins",
  THEME: "bm_theme",
  FONT_SIZE: "bm_font_size",
  LAYOUT: "bm_layout",
  // W8:最近选择的对话工作区(工作区注册表 id;空 = 跟随服务器默认)
  ACTIVE_WORKSPACE: "bm_active_workspace",
} as const;

export type StorageKey = typeof STORAGE_KEYS[keyof typeof STORAGE_KEYS];

export const storage = {
  get(key: StorageKey): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set(key: StorageKey, value: string): void {
    try {
      localStorage.setItem(key, value);
    } catch {
      /* ignore quota or private mode errors */
    }
  },
  remove(key: StorageKey): void {
    try {
      localStorage.removeItem(key);
    } catch {
      /* ignore */
    }
  },
};
