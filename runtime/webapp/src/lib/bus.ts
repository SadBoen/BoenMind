// 2026-09-05 回看收归:此前 8 个跨组件事件以裸字符串 CustomEvent 散落
// 5 个文件,无类型、无常量表,重命名即静默断链。统一在此声明事件名与
// 载荷类型,发布/订阅一律走本模块。

export const BM_EVENTS = {
  /** 新建对话(侧栏/快捷入口触发) */
  chatNew: "bm-chat-new",
  /** 会话列表已变化,侧栏重载 */
  sessionsUpdated: "bm-sessions-updated",
  /** 已切换会话,payload 携带新会话 id */
  sessionSwitched: "bm-session-switched",
  /** 工作区列表已变化,文件树重载 */
  workspacesChanged: "bm-workspaces-changed",
  /** 角色配置已变化 */
  rolesChanged: "bm-roles-changed",
  /** 模型 provider 已变化 */
  providersChanged: "bm-providers-changed",
  /** 打开音乐面板 */
  openMusic: "bm-open-music",
  /** 请求刷新工作区文件 */
  wsRefresh: "bm-ws-refresh",
} as const;

export type BmEventName = (typeof BM_EVENTS)[keyof typeof BM_EVENTS];

/** 会话切换事件的载荷。 */
export interface SessionSwitchedDetail {
  sid: string;
}

export function emit(name: BmEventName, detail?: unknown): void {
  window.dispatchEvent(new CustomEvent(name, { detail }));
}

export function on(
  name: BmEventName,
  handler: (detail: unknown) => void,
): () => void {
  const listener = (ev: Event) => handler((ev as CustomEvent).detail);
  window.addEventListener(name, listener);
  return () => window.removeEventListener(name, listener);
}
