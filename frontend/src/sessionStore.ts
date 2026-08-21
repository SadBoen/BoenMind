import { useSyncExternalStore } from "react";

// 当前会话 id 的模块级 store。
// dockview 面板组件经 portal 渲染，不随父组件 state 重渲染；
// 面板内部用 useSyncExternalStore 订阅本 store，选中/新建会话即时生效。
// 最近会话 id 持久化到 localStorage——刷新/重开后恢复（启动时经 session.list
// 校验 id 仍存在才选中；「启动行为·自动恢复」设置页开关对应此语义）。

let current: string | null = null;
const listeners = new Set<() => void>();

const RECENT_KEY = "bm.recentSession";

export function setCurrentSession(id: string | null) {
  current = id;
  if (id) localStorage.setItem(RECENT_KEY, id);
  else localStorage.removeItem(RECENT_KEY);
  listeners.forEach((l) => l());
}

/** 读取上次会话（可能已失效——调用方需用 session.list 校验）。 */
export function getRecentSession(): string | null {
  return localStorage.getItem(RECENT_KEY);
}

/** 启动是否自动恢复上次会话（设置页「启动行为」开关；默认开）。 */
export function getAutoRestore(): boolean {
  return localStorage.getItem("bm.autoRestore") !== "0";
}

export function setAutoRestore(v: boolean) {
  localStorage.setItem("bm.autoRestore", v ? "1" : "0");
}

/** 启动恢复：session.list 里找最近 id 并选中（开关关 / id 不存在则忽略）。 */
export async function restoreRecentSession(list: { sessionId: string }[]): Promise<void> {
  if (!getAutoRestore()) return;
  const recent = getRecentSession();
  if (recent && list.some((s) => s.sessionId === recent)) {
    current = recent;
    listeners.forEach((l) => l());
  }
}

function getCurrentSession() {
  return current;
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
}

export function useCurrentSession() {
  return useSyncExternalStore(subscribe, getCurrentSession);
}
