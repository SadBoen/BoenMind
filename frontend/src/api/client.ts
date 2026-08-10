/**
 * BoenMind API 客户端。
 *
 * 桌面版（Tauri）与网页版共用同一客户端：网页开发时走 Vite 代理（同源 /api），
 * 桌面版由 Tauri 壳内嵌启动后端并注入后端地址（VITE_API_BASE / window.__BOENMIND_API__）。
 */

export type ProviderKind =
  | "openai"
  | "anthropic"
  | "gemini"
  | "ollama"
  | "llamacpp"
  | "minimax"
  | "deepseek"
  | "openrouter"
  | "custom";

export interface ProviderConfig {
  id: string;
  name: string;
  kind: ProviderKind;
  base_url?: string;
  api_key?: string;
  models: string[];
  default_model?: string;
}

export interface AppConfig {
  providers: ProviderConfig[];
  default_provider?: string;
  default_model?: string;
  working_dir: string;
  theme: string;
}

export interface Session {
  id: string;
  title: string;
  provider_id: string | null;
  model: string | null;
  created_at: number;
  updated_at: number;
}

export interface Message {
  id: number;
  session_id: string;
  role: "user" | "assistant";
  content: string;
  created_at: number;
}

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified: number;
}

export interface HealthInfo {
  status: string;
  version: string;
  workingDir: string;
  providers: number;
  theme: string;
}

export interface PluginInfo {
  id: string;
  name: string;
  description: string;
  kind: string;
  enabled: boolean;
  builtin: boolean;
}

/** 聊天流式事件（对应后端 AgentStreamEvent 的 JSON 序列化） */
export type ChatStreamEvent =
  | { type: "textDelta"; delta: string }
  | { type: "thinkingDelta"; delta: string }
  | { type: "toolCallStart"; name: string }
  | { type: "toolCallDelta"; delta: string }
  | { type: "turnEnd" }
  | { type: "done" }
  | { type: "error"; message: string };

const API_BASE: string = (() => {
  const fromEnv = import.meta.env.VITE_API_BASE as string | undefined;
  if (fromEnv) return fromEnv;
  if (typeof window !== "undefined") {
    const injected = (window as unknown as { __BOENMIND_API__?: string }).__BOENMIND_API__;
    if (injected) return injected;
  }
  return "";
})();

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
  if (!res.ok) {
    let detail = res.statusText;
    try {
      const body = await res.json();
      detail = body.error ?? detail;
    } catch {
      /* ignore */
    }
    throw new Error(detail);
  }
  return res.json() as Promise<T>;
}

export const api = {
  health: () => request<HealthInfo>("/api/health"),

  getConfig: () => request<AppConfig>("/api/config"),
  saveConfig: (config: AppConfig) =>
    request<{ ok: boolean }>("/api/config", { method: "PUT", body: JSON.stringify(config) }),

  listSessions: () => request<Session[]>("/api/sessions"),
  createSession: (body?: { provider_id?: string; model?: string; title?: string }) =>
    request<Session>("/api/sessions", { method: "POST", body: JSON.stringify(body ?? {}) }),
  getSession: (id: string) =>
    request<{ session: Session; messages: Message[] }>(`/api/sessions/${id}`),
  renameSession: (id: string, title: string) =>
    request<{ ok: boolean }>(`/api/sessions/${id}`, {
      method: "PATCH",
      body: JSON.stringify({ title }),
    }),
  deleteSession: (id: string) =>
    request<{ ok: boolean }>(`/api/sessions/${id}`, { method: "DELETE" }),

  listPlugins: () => request<PluginInfo[]>("/api/plugins"),
  setPlugin: (id: string, enabled: boolean) =>
    request<{ ok: boolean }>(`/api/plugins/${id}`, {
      method: "POST",
      body: JSON.stringify({ enabled }),
    }),
  installPlugin: (path: string) =>
    request<PluginInfo>("/api/plugins/install", {
      method: "POST",
      body: JSON.stringify({ path }),
    }),

  listWorkspace: (dir = "") =>
    request<{ dir: string; entries: FileEntry[] }>(
      `/api/workspace/list?dir=${encodeURIComponent(dir)}`,
    ),
  readFile: (path: string) =>
    request<{
      name: string;
      path: string;
      mime: string;
      kind: "text" | "binary";
      content: string;
      size: number;
    }>(`/api/workspace/file?path=${encodeURIComponent(path)}`),

  /**
   * 流式对话。返回一个可取消的响应对象：
   *  - `onEvent` 收到每个 ChatStreamEvent
   *  - `close()` 中断连接
   */
  chat: (sessionId: string, message: string, onEvent: (ev: ChatStreamEvent) => void) => {
    const controller = new AbortController();
    const done = new Promise<void>((resolve) => {
      (async () => {
        try {
          const res = await fetch(`${API_BASE}/api/chat`, {
            method: "POST",
            headers: { "Content-Type": "application/json", Accept: "text/event-stream" },
            body: JSON.stringify({ session_id: sessionId, message }),
            signal: controller.signal,
          });
          if (!res.ok || !res.body) {
            const body = await res.json().catch(() => null);
            throw new Error(body?.error ?? `请求失败: ${res.status}`);
          }
          const reader = res.body.getReader();
          const decoder = new TextDecoder();
          let buffer = "";
          while (true) {
            const { done: streamDone, value } = await reader.read();
            if (streamDone) break;
            buffer += decoder.decode(value, { stream: true });
            // SSE 事件以空行分隔
            let sep: number;
            while ((sep = buffer.indexOf("\n\n")) !== -1) {
              const raw = buffer.slice(0, sep);
              buffer = buffer.slice(sep + 2);
              for (const line of raw.split("\n")) {
                if (line.startsWith("data: ")) {
                  onEvent(JSON.parse(line.slice(6)) as ChatStreamEvent);
                }
              }
            }
          }
        } catch (err) {
          if (!controller.signal.aborted) {
            onEvent({ type: "error", message: String(err) });
          }
        } finally {
          resolve();
        }
      })();
    });
    return { done, close: () => controller.abort() };
  },
};
