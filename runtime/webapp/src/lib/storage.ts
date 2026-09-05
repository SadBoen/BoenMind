// 统一前端 localStorage 键常量与安全存取器(消除散落的魔法字符串与拼写错误)

export const STORAGE_KEYS = {
  ACTIVE_MODEL: "bm_active_model",
  ACTIVE_ROLE: "bm_active_role",
  SESSION: "bm_session",
  SESSIONS: "bm_sessions",
  PINS: "bm_pins",
  THEME: "bm_theme",
  FONT_SIZE: "bm_font_size",
  LAYOUT: "bm_layout",
  // W8:最近选择的对话工作区(工作区注册表 id;空 = 跟随服务器默认)
  ACTIVE_WORKSPACE: "bm_active_workspace",
  // 插件中心表格列宽
  PLUGINS_TABLE_COLS: "bm_plugins_table_cols",
  // 权限模式:ask(变更前确认)|plan(计划模式)|yolo(完全访问)
  PERMISSION_MODE: "bm_permission_mode",
} as const;

export type PermissionMode = "ask" | "plan" | "yolo";

export interface SessionItemMeta {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
}

export const sessionsStore = {
  list(): SessionItemMeta[] {
    try {
      const raw = storage.get(STORAGE_KEYS.SESSIONS);
      if (!raw) return [];
      const parsed = JSON.parse(raw);
      return Array.isArray(parsed) ? parsed : [];
    } catch {
      return [];
    }
  },
  save(list: SessionItemMeta[]): void {
    storage.set(STORAGE_KEYS.SESSIONS, JSON.stringify(list));
  },
  upsert(id: string, title?: string): SessionItemMeta[] {
    const list = this.list();
    const now = Date.now();
    const existing = list.find((s) => s.id === id);
    if (existing) {
      if (title && (!existing.title || existing.title === "新对话")) {
        existing.title = title;
      }
      existing.updatedAt = now;
      this.save(list);
      return list;
    }
    const newItem: SessionItemMeta = {
      id,
      title: title || "新对话",
      createdAt: now,
      updatedAt: now,
    };
    const next = [newItem, ...list];
    this.save(next);
    return next;
  },
  remove(id: string): SessionItemMeta[] {
    const next = this.list().filter((s) => s.id !== id);
    this.save(next);
    return next;
  },
};

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
