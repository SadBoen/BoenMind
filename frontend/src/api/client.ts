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
  /** 插件权限档位（safe / balanced / permissive） */
  extension_policy?: string;
  /** YOLO 开关：放行 exec/env 等危险能力 */
  extension_allow_dangerous?: boolean;
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

/** 代理提交的改进建议（refine-suggest；审批后生效，skill 类型可回滚） */
export interface RefinementSuggestion {
  id: string;
  session_id: string | null;
  /** "skill:<id>" 或 "system_prompt" */
  target: string;
  /** 目标描述中需修改的原文片段 */
  quote: string;
  /** 建议的替换/追加文本 */
  suggested: string;
  reason: string;
  /** pending | approved | rejected */
  status: string;
  created_at: number;
  applied_at: number | null;
  backup_path: string | null;
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

/** 一次 prompt 回合的任务记录（断线续跑 + 心跳进度） */
export interface Task {
  id: string;
  session_id: string;
  /** running | completed | failed | cancelled */
  status: string;
  /** 心跳进度文本（最近活动摘要） */
  progress: string;
  started_at: number;
  updated_at: number;
  finished_at: number | null;
  error: string | null;
}

/** 自更新（热升级）检查结果 */
export interface UpdateCheckInfo {
  current: string;
  latest: {
    version: string;
    notes: string;
    asset: { name: string; url: string; size: number; sig_url: string };
  } | null;
}

/** 应用更新结果 */
export interface ApplyUpdateResult {
  version: string;
  /** managed（桌面壳子进程：落盘 runtime 目录，由壳重启）| standalone（已替换自身，调 restart 生效） */
  mode: "managed" | "standalone";
  path?: string;
}

/** 聊天流式事件（对应后端 AgentStreamEvent 的 JSON 序列化） */
export type ChatStreamEvent =
  | { type: "textDelta"; delta: string }
  | { type: "toolCallStart"; id: string; name: string; args: unknown }
  | { type: "toolCallEnd"; id: string; name: string; isError: boolean }
  | {
      type: "permissionRequest";
      id: string;
      extensionId?: string;
      capability: string;
      message: string;
    }
  | { type: "taskProgress"; progress: string }
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

// ── 访问令牌（服务器部署 BOENMIND_TOKEN 守卫）──
// 令牌持久化在 localStorage；未设置时行为与桌面版一致（不带 Authorization 头）。
let authToken: string = (() => {
  try {
    return localStorage.getItem("boenmind.token") ?? "";
  } catch {
    return "";
  }
})();
const TOKEN_KEY = "boenmind.token";

/** 设置/清除访问令牌（保存到 localStorage，立即对后续请求生效） */
export function setAuthToken(token: string) {
  authToken = token.trim();
  try {
    if (authToken) localStorage.setItem(TOKEN_KEY, authToken);
    else localStorage.removeItem(TOKEN_KEY);
  } catch {
    /* ignore */
  }
}

/** 401（unauthorized）回调：前端据此弹出令牌输入框 */
let unauthorizedHandler: (() => void) | null = null;
export function onUnauthorized(handler: (() => void) | null) {
  unauthorizedHandler = handler;
}

function notifyUnauthorized() {
  unauthorizedHandler?.();
}

function authHeaders(): Record<string, string> {
  return authToken ? { Authorization: `Bearer ${authToken}` } : {};
}

/** 统一错误解析：401 unauthorized 触发令牌回调，其余透传服务端 error 详情 */
async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...init,
    // headers 在 init 之后显式合并：init 里若带 headers 会被按调用方意图覆盖
    headers: { "Content-Type": "application/json", ...authHeaders(), ...init?.headers },
  });
  if (!res.ok) {
    let detail = res.statusText;
    let unauthorized = false;
    try {
      const body = await res.json();
      detail = body.error ?? detail;
      unauthorized = res.status === 401 && body.error === "unauthorized";
    } catch {
      /* ignore */
    }
    if (unauthorized) notifyUnauthorized();
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

  /**
   * 官方端点表（服务商端点单一数据源在后端；旧后端/失败时返回 null，
   * 前端保留本地预设兜底——见 provider-presets.tsx 的 applyApiPresets）。
   */
  providerPresets: async (): Promise<Record<string, string | null> | null> => {
    try {
      const data = await request<{ presets: [string, string | null][] }>(
        "/api/providers/presets",
      );
      return Object.fromEntries(data.presets);
    } catch {
      return null;
    }
  },

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
  /** 按包源安装插件（npm:包名 / git:host/owner/repo / 本地路径）；一个包可含多个扩展 */
  installPluginFromSource: (source: string) =>
    request<PluginInfo[]>("/api/plugins/install-source", {
      method: "POST",
      body: JSON.stringify({ source }),
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

  listRefinementSuggestions: (status?: string) =>
    request<RefinementSuggestion[]>(
      `/api/refinement-suggestions${status ? `?status=${status}` : ""}`,
    ),
  approveRefinementSuggestion: (id: string) =>
    request<{ ok: boolean; backup: string | null }>(
      `/api/refinement-suggestions/${id}/approve`,
      { method: "POST" },
    ),
  rejectRefinementSuggestion: (id: string) =>
    request<{ ok: boolean }>(`/api/refinement-suggestions/${id}/reject`, {
      method: "POST",
    }),
  rollbackRefinementSuggestion: (id: string) =>
    request<{ ok: boolean }>(`/api/refinement-suggestions/${id}/rollback`, {
      method: "POST",
    }),

  listSessionTasks: (sessionId: string) =>
    request<Task[]>(`/api/sessions/${sessionId}/tasks`),

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
            headers: {
              "Content-Type": "application/json",
              Accept: "text/event-stream",
              ...authHeaders(),
            },
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
            if (res.status === 401 && body?.error === "unauthorized") notifyUnauthorized();
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
                // 宽容解析：`data:` 后可选空格（服务端格式微变不致断流）
                if (line.startsWith("data:")) {
                  try {
                    onEvent(JSON.parse(line.slice(5).trim()) as ChatStreamEvent);
                  } catch {
                    /* 跳过无法解析的 data 行 */
                  }
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
  /** 插件权限询问决策回传（允许一次/拒绝/总是允许-拒绝） */
  respondPermission: (requestId: string, allow: boolean, always: boolean) =>
    request<{ ok: boolean }>("/api/chat/permission-response", {
      method: "POST",
      body: JSON.stringify({ request_id: requestId, allow, always }),
    }),

  /** 检查更新（用户手动触发；不做任何自动检查） */
  checkUpdate: () => request<UpdateCheckInfo>("/api/updates/check"),

  /** 下载并应用更新：验签后 managed 落盘 runtime 目录 / standalone 替换自身 */
  applyUpdate: () => request<ApplyUpdateResult>("/api/updates/apply", { method: "POST" }),

  /** standalone（Linux 部署）重启生效：进程 exec 新版（PID 不变）；managed 由壳重启，勿调 */
  restartUpdate: () =>
    request<{ status: string }>("/api/updates/restart", { method: "POST" }),
};
