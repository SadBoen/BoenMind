// 统一前端 localStorage 键常量与安全存取器(消除散落的魔法字符串与拼写错误)

// 一次性退役清理(2026-09-08 三端一致批):会话目录已收归服务端
// (GET /admin/sessions),本地老账 bm_sessions 不再读取,清掉残留
try {
  localStorage.removeItem("bm_sessions");
} catch {
  /* ignore */
}

// 会话指针随页面寿命(2026-09-08 三点会话语义,用户裁决):打开网址/刷新
// = 全新临时会话,不恢复上次会话视图(要回历史点左侧列表);临时会话在
// 首条消息发出前服务端不存在,未发消息即关页/切走 = 什么都不留。
// 指针仅在页面存活期内使用(首发消息/切会话时写入)。
try {
  localStorage.removeItem("bm_session");
} catch {
  /* ignore */
}

export const STORAGE_KEYS = {
  ACTIVE_MODEL: "bm_active_model",
  ACTIVE_ROLE: "bm_active_role",
  SESSION: "bm_session",
  THEME: "bm_theme",
  FONT_SIZE: "bm_font_size",
  LAYOUT: "bm_layout",
  // W8:最近选择的对话工作区(工作区注册表 id;空 = 跟随服务器默认)
  ACTIVE_WORKSPACE: "bm_active_workspace",
  // 插件中心表格列宽
  PLUGINS_TABLE_COLS: "bm_plugins_table_cols",
  // 权限模式(ADR-0030):状态权威在服务端会话;此键仅存"新对话尚未
  // 创建时"的暂存意向,会话创建后上报服务端一次即清除,不作裁决依据
  PERMISSION_MODE: "bm_permission_mode",
  // 思考等级:off(关闭)|low(轻度)|medium(中度)|high(深度)
  THINKING_LEVEL: "bm_thinking_level",
} as const;

export type PermissionMode = "ask" | "plan" | "yolo";
export type ThinkingLevel = "off" | "low" | "medium" | "high";

// 会话目录条目形态(2026-09-08 三端一致批起:数据源 = GET /admin/sessions,
// 服务端权威;此类型仅作面板渲染视图)。时间已折算为 epoch ms。
export interface SessionItemMeta {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
}

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
