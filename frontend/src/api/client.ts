/**
 * BoenMind API 客户端。
 *
 * 桌面版（Tauri）与网页版共用同一客户端：网页开发时走 Vite 代理（同源 /api），
 * 桌面版由 Tauri 壳内嵌启动后端并注入后端地址（VITE_API_BASE / window.__BOENMIND_API__）。
 */

import i18n from "@/i18n";

export type ProviderKind =
  | "openai"
  | "anthropic"
  | "gemini"
  | "ollama"
  | "llamacpp"
  | "minimax"
  | "deepseek"
  | "openrouter"
  | "moonshot"
  | "zhipu"
  | "qwen"
  | "xai"
  | "zai"
  | "groq"
  | "mistral"
  | "together"
  | "cerebras"
  | "fireworks"
  | "huggingface"
  | "nvidia"
  | "xiaomi"
  | "antling"
  | "baseten"
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
  lang: string;
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
  /** 该助手消息关联的工具调用（后端持久化，回放展示） */
  tool_calls?: ToolCall[];
}

/** 一次工具调用记录（对应后端 db::ToolCall） */
export interface ToolCall {
  seq: number;
  tool_name: string;
  args: unknown;
  is_error: boolean;
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
  lang: string;
}

/** 插件设置页 schema 的字段类型 */
export type SettingFieldType = "string" | "secret" | "boolean" | "number" | "select" | "group";

/** manifest settings 数组里一个字段的声明（后端 SettingField 的 JSON 序列化） */
export interface SettingField {
  key: string;
  type: SettingFieldType;
  label: string;
  description?: string;
  options?: string[];
  min?: number;
  max?: number;
  default?: string | number | boolean;
  /** group 类型：子字段模板（key 为相对 key） */
  fields?: SettingField[];
  /** group 类型：默认实例数 */
  instances?: number;
  /** 组的显示名（组内第一个字段声明），分组卡片标题用 */
  groupLabel?: string;
}

/** 插件用量（quota.json 中单个源的统计） */
export interface QuotaInfo {
  used: number;
  total: number;
  unit: string;
  reset?: string;
  exhaustedAt?: number;
  callsToday?: number;
}

/** 插件用量声明（manifest `quota` 段）：文件路径 + 按次计费源清单 */
export interface QuotaDecl {
  path: string;
  countOnTest?: string[];
}

/** 设置页「测试」按钮的探测模板（manifest `testSources` 段，单个源） */
export interface TestSourceDecl {
  method: string;
  url: string;
  headers?: Record<string, string>;
  body?: unknown;
  label?: string;
}

export interface PluginInfo {
  id: string;
  name: string;
  description: string;
  kind: string;
  enabled: boolean;
  builtin: boolean;
  /** 插件设置页 schema（无设置页的插件为 undefined） */
  settingsSchema?: SettingField[];
  /** 用量声明（manifest quota 段；无用量统计的插件为 undefined） */
  quota?: QuotaDecl;
  /** 可测试源模板（manifest testSources 段；无测试按钮的插件为 undefined） */
  testSources?: Record<string, TestSourceDecl>;
}

export interface SkillInfo {
  id: string;
  name: string;
  description: string;
  owner?: string;
  repo?: string;
  /** registry（skills.sh）/ local */
  source: string;
  enabled: boolean;
}

/** skills.sh 随机抽取的候选（尚未安装） */
export interface SkillCandidate {
  skill_id: string;
  name: string;
  description: string;
  owner: string;
  repo: string;
  url: string;
}

/** 聊天流式事件（对应后端 AgentStreamEvent 的 JSON 序列化） */
export type ChatStreamEvent =
  | { type: "textDelta"; delta: string }
  | { type: "toolCallStart"; id: string; name: string; args: unknown }
  | { type: "toolCallEnd"; id: string; name: string; isError: boolean }
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

  /** 向提供商接口拉取模型列表（表单临时填写的端点/key，不落盘） */
  listProviderModels: (body: { kind: ProviderKind; base_url?: string; api_key?: string }) =>
    request<{ models: string[] }>("/api/providers/list-models", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  /** 测试提供商连接；message 为空仅测连通，非空发送真实对话 */
  testProvider: (body: {
    kind: ProviderKind;
    base_url?: string;
    api_key?: string;
    model?: string;
    message?: string;
  }) =>
    request<{ ok: boolean; detail: string }>("/api/providers/test", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  listSessions: () => request<Session[]>("/api/sessions"),

  /** 某模型的可用思考档位（后端按 pi 白名单判定；请求失败前端用默认 4 档兜底） */
  thinkingLevels: (provider: string, model: string) =>
    request<{ levels: string[] }>(
      `/api/thinking-levels?provider=${encodeURIComponent(provider)}&model=${encodeURIComponent(model)}`,
    ),
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
  uninstallPlugin: (id: string) =>
    request<{ ok: boolean }>(`/api/plugins/${id}`, { method: "DELETE" }),

  /** 插件设置（secret 字段已掩码回显）；quota 为插件在工作文件夹下的用量（可选） */
  getPluginSettings: (id: string) =>
    request<{
      settings: Record<string, string | number | boolean>;
      quota?: Record<string, QuotaInfo> | null;
    }>(`/api/plugins/${id}/settings`),
  /** 保存插件设置（secret 提交空/掩码 = 保留原值；__clear.<key>=true 清除），返回合并后的掩码版 */
  putPluginSettings: (id: string, values: Record<string, string | number | boolean>) =>
    request<{ ok: boolean; settings: Record<string, string | number | boolean> }>(
      `/api/plugins/${id}/settings`,
      { method: "PUT", body: JSON.stringify({ values }) },
    ),
  /**
   * 测试插件某个源（jina/tavily/exa/serper/firecrawl/custom1…）的连通性。
   * values 为表单当前值（未保存的修改一并探测；secret 空/掩码 = 已存原值）。
   * 测试成功消耗 1 次免费额度，响应附带最新 quota。
   */
  testPluginSource: (
    id: string,
    source: string,
    values?: Record<string, string | number | boolean>,
  ) =>
    request<{
      ok: boolean;
      latencyMs: number;
      detail: string;
      quota?: Record<string, QuotaInfo> | null;
    }>(`/api/plugins/${id}/test-source`, {
      method: "POST",
      body: JSON.stringify({ source, values: values ?? {} }),
    }),

  listSkills: () => request<SkillInfo[]>("/api/skills"),
  setSkill: (id: string, enabled: boolean) =>
    request<{ ok: boolean }>(`/api/skills/${id}`, {
      method: "POST",
      body: JSON.stringify({ enabled }),
    }),
  uninstallSkill: (id: string) =>
    request<{ ok: boolean }>(`/api/skills/${id}`, { method: "DELETE" }),
  installSkill: (body: { owner: string; repo: string; skill_id: string } | { path: string }) =>
    request<SkillInfo>("/api/skills/install", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  randomSkills: (count = 5) =>
    request<SkillCandidate[]>(`/api/skills/registry/random?count=${count}`),

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
   *  - `close()` 中断连接（后端检测到断开会自动取消 prompt）
   *  - `model`/`thinking` 可选，对当前会话即时切换（不改变会话记录）
   */
  chat: (
    sessionId: string,
    message: string,
    onEvent: (ev: ChatStreamEvent) => void,
    opts?: { model?: string; thinking?: string },
  ) => {
    const controller = new AbortController();
    const done = new Promise<void>((resolve) => {
      (async () => {
        try {
          const res = await fetch(`${API_BASE}/api/chat`, {
            method: "POST",
            headers: { "Content-Type": "application/json", Accept: "text/event-stream" },
            body: JSON.stringify({
              session_id: sessionId,
              message,
              model: opts?.model,
              thinking: opts?.thinking,
            }),
            signal: controller.signal,
          });
          if (!res.ok || !res.body) {
            const body = await res.json().catch(() => null);
            throw new Error(body?.error ?? i18n.t("api.requestFailed", { status: res.status }));
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

  /**
   * 取消进行中的流式对话。后端触发 pi AbortSignal，prompt 尽快返回，
   * 已生成的部分文本照常入库并下发 done（不中断 SSE 连接，等 done 固化内容）。
   */
  stopChat: (sessionId: string) =>
    request<{ ok: boolean }>("/api/chat/stop", {
      method: "POST",
      body: JSON.stringify({ session_id: sessionId }),
    }),
};
