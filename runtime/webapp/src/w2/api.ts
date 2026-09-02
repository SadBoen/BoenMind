// W2 管理面 REST 客户端(壳子私用;/admin/* 见后端 webadmin.rs 模块注释:
// 暂不入冻结合同,行为规格 = bm-surface-http tests/webadmin_tests.rs)。
// 错误形状统一 {error:{message}};探针/连通类结果走 200 + ok 布尔。

export type Provider = {
  id: string;
  name: string;
  baseUrl: string;
  models: string[];
  defaultModel?: string;
  secretSet: boolean;
};

export type ProviderInput = {
  name: string;
  baseUrl: string;
  apiKey?: string;
  models?: string[];
  defaultModel?: string;
};

export type ProbeResult = {
  ok: boolean;
  status?: number;
  latencyMs: number;
  models?: string[];
  error?: string;
};

export type McpServer = {
  name: string;
  transport: "stdio";
  command: string;
  args: string[];
  env?: Record<string, string>;
  tool_timeout_ms?: number;
  restart_limit?: number;
};

export type McpListResult = {
  file: string;
  servers: McpServer[];
  entries?: {
    server: McpServer;
    manifest?: {
      title?: string;
      description?: string;
      config_schema?: {
        key: string;
        label: string;
        hint?: string;
        type: "string" | "secret" | "range" | "select";
        default?: string | number;
        min?: number;
        max?: number;
        unit?: string;
        options?: { value: string; label: string }[];
      }[];
    } | null;
    config?: Record<string, unknown>;
  }[];
  loadedAtBoot: string[];
  note: string;
};

export type McpReloadResult = {
  ok: boolean;
  registered: string[];
  failed: { name: string; error: string }[];
  note: string;
};

export type Capability = {
  name: string;
  provider?: string;
  effect?: string;
  idempotent?: boolean;
  approval?: string;
};

export type FsEntry = { name: string; kind: "dir" | "file"; size: number | null };

export type RoleItem = {
  id: string;
  name: string;
  description?: string;
  system_prompt: string;
  skills?: string[];
};

// W4b Skill 知识包(合同 capability/skill.v0_1;只是数据,加载不改变权限)
export type SkillItem = {
  skill_id: string;
  name: string;
  description?: string | null;
  instruction: string;
  allowed_capabilities?: string[];
};

export type RolesResponse = {
  ok: boolean;
  active_id: string;
  roles: RoleItem[];
};

// W5:一次模型调用的上下文快照(/admin/context 行;服务端已做凭据脱敏与
// 单条内容 16K 字符截断)
export type CtxStep = {
  seq: number;
  ts: string;
  session_id: string;
  agent_id: string;
  operation_id: string;
  turn_index: number;
  step: number;
  attempt?: number;
  model_id: string;
  streaming: boolean;
  messages: { role: string; content: string; content_truncated?: boolean }[];
  tools: { function?: { name?: string; description?: string } }[];
  status: "ok" | "error" | "cancelled";
  error_code?: string | null;
  tokens_in?: number | null;
  tokens_out?: number | null;
  latency_ms?: number | null;
};

async function req<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, init);
  const body = await res.json().catch(() => null);
  if (!res.ok) {
    const msg =
      (body as { error?: { message?: string } } | null)?.error?.message ??
      `HTTP ${res.status}`;
    throw new Error(msg);
  }
  return body as T;
}

const json = (method: string, body: unknown): RequestInit => ({
  method,
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify(body),
});

export const api = {
  providers: {
    list: () => req<{ providers: Provider[] }>("/admin/providers"),
    create: (b: ProviderInput) =>
      req<{ provider: Provider }>("/admin/providers", json("POST", b)),
    update: (id: string, b: ProviderInput) =>
      req<{ provider: Provider }>(`/admin/providers/${id}`, json("PUT", b)),
    remove: (id: string) =>
      req<{ ok: boolean }>(`/admin/providers/${id}`, { method: "DELETE" }),
    probe: (baseUrl: string, apiKey?: string) =>
      req<ProbeResult>("/admin/providers/probe", json("POST", { baseUrl, apiKey })),
    setActive: (providerId: string, modelId?: string) =>
      req<{ ok: boolean; restartRequired: boolean; note: string }>(
        "/admin/model/active",
        json("PUT", { providerId, modelId }),
      ),
  },
  activeModel: () =>
    req<{ values: Record<string, unknown>; secret_set: Record<string, boolean> }>(
      "/admin/model/active",
    ),
  mcp: {
    list: () => req<McpListResult>("/admin/mcp"),
    create: (b: Partial<McpServer>) => req<{ ok: boolean }>("/admin/mcp", json("POST", b)),
    update: (name: string, b: Partial<McpServer>) =>
      req<{ ok: boolean }>(`/admin/mcp/${name}`, json("PUT", b)),
    remove: (name: string) =>
      req<{ ok: boolean }>(`/admin/mcp/${name}`, { method: "DELETE" }),
    reload: () =>
      req<McpReloadResult>("/admin/mcp/reload", { method: "POST" }),
    candidates: () =>
      req<{
        ok: boolean;
        dir: string;
        candidates: {
          file: string;
          name: string;
          title: string;
          description: string;
          registered: boolean;
        }[];
        note: string;
      }>("/admin/mcp/candidates", { method: "POST" }),
    approve: (name: string) =>
      req<{ ok: boolean; note: string }>(
        "/admin/mcp/approve",
        json("POST", { name }),
      ),
    test: (name: string) =>
      req<{ ok: boolean; name: string; tools?: number; error?: string }>(
        `/admin/mcp/test/${name}`,
        { method: "POST" },
      ),
    status: () =>
      req<{
        status: { name: string; ok: boolean; tools?: number; error?: string }[];
      }>("/admin/mcp/status"),
    getConfig: (name: string) =>
      req<{ name: string; values: Record<string, unknown> }>(
        `/admin/mcp-config/${name}`,
      ),
    saveConfig: (name: string, values: Record<string, unknown>) =>
      req<{ ok: boolean; note: string }>(
        `/admin/mcp-config/${name}`,
        json("PUT", { values }),
      ),
  },
  roles: {
    get: () => req<RolesResponse>("/admin/roles"),
    save: (role: Partial<RoleItem> & { id: string; set_active?: boolean }) =>
      req<{ ok: boolean; note: string; active_id: string }>("/admin/roles", json("POST", role)),
    delete: (id: string) =>
      req<{ ok: boolean; note: string; active_id: string }>(`/admin/roles/${encodeURIComponent(id)}`, {
        method: "DELETE",
      }),
    setActive: (id: string) =>
      req<{ ok: boolean; note: string; active_id: string }>(
        `/admin/roles/active/${encodeURIComponent(id)}`,
        { method: "PUT" },
      ),
  },
  skills: {
    list: () => req<{ ok: boolean; skills: SkillItem[] }>("/admin/skills"),
    save: (skill: SkillItem) =>
      req<{ ok: boolean; note: string }>("/admin/skills", json("POST", skill)),
    remove: (id: string) =>
      req<{ ok: boolean; note: string }>(
        `/admin/skills/${encodeURIComponent(id)}`,
        { method: "DELETE" },
      ),
  },
  logs: () =>
    req<{ ok: boolean; exec: string[]; events: string[]; context: string[] }>("/admin/logs"),
  // W5 上下文透视:模型调用请求快照(context-log.jsonl 尾部,最旧在前)
  context: () => req<{ ok: boolean; steps: CtxStep[] }>("/admin/context"),
  capabilities: () =>
    req<{
      builtin: Capability[];
      mcp: { name: string; tools: number | null; loaded: boolean; pendingRemoval?: boolean }[];
    }>(
      "/admin/capabilities",
    ),
  fs: {
    list: (path: string) =>
      req<{ path: string; entries: FsEntry[]; root: string }>(
        `/admin/fs/list?path=${encodeURIComponent(path)}`,
      ),
    file: (path: string) =>
      req<{ path: string; name: string; size: number; content: string }>(
        `/admin/fs/file?path=${encodeURIComponent(path)}`,
      ),
  },
};
