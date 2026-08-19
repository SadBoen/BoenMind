import { useSyncExternalStore } from "react";

// 当前会话 id 的模块级 store。
// dockview 面板组件经 portal 渲染，不随父组件 state 重渲染；
// 面板内部用 useSyncExternalStore 订阅本 store，选中/新建会话即时生效。

let current: string | null = null;
const listeners = new Set<() => void>();

export function setCurrentSession(id: string | null) {
  current = id;
  listeners.forEach((l) => l());
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
