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
  loadedAtBoot: string[];
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
  },
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
