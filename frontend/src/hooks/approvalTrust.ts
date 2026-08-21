import { useSyncExternalStore } from "react";

// 审批豁免表（模块级 store）：key = sessionId → 已信任的工具名数组。
// 「本会话信任该工具」→ trustTool() 记入；ApprovalModal 命中豁免自动放行。
// SettingsPage 管理面读取/清除。纯内存态（页面刷新失效，贴合"会话级"语义）。

let trusted: Record<string, string[]> = {};
const listeners = new Set<() => void>();

function emit() {
  listeners.forEach((l) => l());
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
}

function getSnapshot() {
  return trusted;
}

export function trustTool(sessionId: string, toolName: string) {
  trusted = {
    ...trusted,
    [sessionId]: trusted[sessionId] ? [...trusted[sessionId], toolName] : [toolName],
  };
  emit();
}

export function clearTrusted(sessionId?: string) {
  if (sessionId) {
    trusted = { ...trusted, [sessionId]: [] };
  } else {
    trusted = {};
  }
  emit();
}

export function isTrusted(sessionId: string, toolName: string): boolean {
  return (trusted[sessionId] ?? []).includes(toolName);
}

/** 订阅豁免表（组件读最新值 + 跟随更新）。 */
export function useApprovalTrust(): Record<string, string[]> {
  return useSyncExternalStore(subscribe, getSnapshot);
}