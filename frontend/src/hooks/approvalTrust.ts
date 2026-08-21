import { useSyncExternalStore } from "react";

// 审批豁免表（模块级 store）：key = sessionId → 已信任的工具名数组。
// 「本会话信任该工具」→ trustTool() 记入；ApprovalModal 命中豁免自动放行。
// SettingsPage 管理面读取/清除。
// 持久化到 localStorage（bm.approvalTrust）——刷新/重启保留，跨会话长期生效。

const STORAGE_KEY = "bm.approvalTrust";

function load(): Record<string, string[]> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const out: Record<string, string[]> = {};
    for (const [k, v] of Object.entries(parsed)) {
      if (Array.isArray(v)) out[k] = v.filter((x): x is string => typeof x === "string");
    }
    return out;
  } catch {
    return {};
  }
}

let trusted: Record<string, string[]> = load();
const listeners = new Set<() => void>();

function persist() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(trusted));
  } catch {
    // localStorage 不可用（隐私模式等）时降级为内存态，不阻断审批流
  }
}

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
  const cur = trusted[sessionId] ?? [];
  if (cur.includes(toolName)) return;
  trusted = { ...trusted, [sessionId]: [...cur, toolName] };
  persist();
  emit();
}

export function clearTrusted(sessionId?: string) {
  if (sessionId) {
    trusted = { ...trusted, [sessionId]: [] };
  } else {
    trusted = {};
  }
  persist();
  emit();
}

export function isTrusted(sessionId: string, toolName: string): boolean {
  return (trusted[sessionId] ?? []).includes(toolName);
}

/** 订阅豁免表（组件读最新值 + 跟随更新）。 */
export function useApprovalTrust(): Record<string, string[]> {
  return useSyncExternalStore(subscribe, getSnapshot);
}