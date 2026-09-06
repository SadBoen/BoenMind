//! 插件中心共享类型与 Draft 转换(自 PluginsPage.tsx 机械移入)。
import { type McpServer, type ProviderManifestItem } from "../api";

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
