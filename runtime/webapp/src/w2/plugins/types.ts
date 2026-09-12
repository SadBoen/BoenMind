//! 插件中心共享类型与 Draft 转换(自 PluginsPage.tsx 机械移入)。
import { type McpServer, type ProviderManifestItem, type WasmPlugin } from "../api";

export type ToolInfo = {
  name: string;
  description?: string;
};

export type TablePluginItem = {
  id: string;
  name: string;
  type: "builtin" | "external" | "wasm";
  detail: string;
  tools: ToolInfo[];
  isOnline?: boolean;
  serverRef?: McpServer;
  /** ADR-0023:官方随包来源但最新官方清单已不含 → 建议删除 */
  deprecated?: boolean;
  /** ADR-0042:wasm 插件声明条目(卸载按 capability 摘除) */
  wasmRef?: WasmPlugin;
  /** ADR-0045:provider 声明的插件身份(tool/connector);展示用,不参与分类 */
  pluginKind?: string | null;
  pluginId?: string | null;
  pluginVersion?: string | null;
};

export type McpManifestSchemaItem = {
  key: string;
  label: string;
  hint?: string;
  type: "string" | "secret" | "range" | "select" | "providers";
  default?: string | number;
  min?: number;
  max?: number;
  unit?: string;
  options?: { value: string; label: string }[];
  /** providers 类型专用:内置默认模板 */
  items?: ProviderManifestItem[];
};

export type ProviderEntry = ProviderManifestItem & {
  /** 是否已在「已配置」列表中(用于下拉区分) */
  present?: boolean;
};

export type ConfigTarget = {
  name: string;
  server?: McpServer;
  schema: McpManifestSchemaItem[];
  values: Record<string, unknown>;
};

export type McpCandidatesResult = {
  ok: boolean;
  dir: string;
  bundled_dir?: string | null;
  candidates: {
    file: string;
    name: string;
    title: string;
    description: string;
    registered: boolean;
    source?: string;
    /** 在删除名单(墓碑)中:批准接入即恢复 */
    tombstoned?: boolean;
  }[];
  note: string;
};

export type Draft = {
  name: string;
  transport: "stdio" | "sse" | "http" | "streamable-http";
  url: string;
  bearer_token: string;
  command: string;
  args: string;
  env: string;
  tool_timeout_ms: string;
  _editing?: boolean;
};

export const emptyDraft: Draft = {
  name: "",
  transport: "stdio",
  url: "",
  bearer_token: "",
  command: "",
  args: "",
  env: "",
  tool_timeout_ms: "",
};

export function toDraft(s: McpServer): Draft {
  const validTransports = ["http", "sse", "streamable-http"];
  return {
    name: s.name,
    transport: validTransports.includes(s.transport) ? s.transport : "stdio",
    url: s.url ?? "",
    bearer_token: s.bearer_token ?? "",
    command: s.command ?? "",
    args: (s.args ?? []).join(" "),
    env: Object.entries(s.env ?? {})
      .map(([k, v]) => `${k}=${v}`)
      .join("\n"),
    tool_timeout_ms: s.tool_timeout_ms ? String(s.tool_timeout_ms) : "",
    _editing: true,
  };
}

export function fromDraft(d: Draft): Partial<McpServer> {
  const env: Record<string, string> = {};
  for (const line of d.env.split("\n")) {
    const i = line.indexOf("=");
    if (i <= 0) continue;
    const k = line.slice(0, i).trim();
    const v = line.slice(i + 1).trim();
    if (k && v) env[k] = v;
  }
  const base: Partial<McpServer> = {
    name: d.name.trim(),
    transport: d.transport,
    env,
    ...(d.tool_timeout_ms ? { tool_timeout_ms: Number(d.tool_timeout_ms) } : {}),
  };
  if (d.transport === "stdio") {
    base.command = d.command.trim();
    base.args = d.args.split(/\s+/).filter(Boolean);
  } else {
    base.url = d.url.trim();
    if (d.bearer_token.trim()) {
      base.bearer_token = d.bearer_token.trim();
    }
  }
  return base;
}
