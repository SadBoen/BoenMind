/**
 * MCP 设置页（bm-mcp 官方插件）：外部 MCP server 管理——
 * 已连接 server 状态（协议版本/工具数）+ 连接/断开/编辑（即时生效
 * 并持久化到 config.toml）+ 标准配置自动发现提示。
 *
 * 编辑 = 回填 config.toml 配置 → 保存（同名先断开重连），env/headers
 * 是 KEY 等敏感配置的载体。配置也可直接放 config.toml 的 `mcp` 数组，
 * 或项目 `.mcp.json`（Claude Code 格式）/ ~/.agents/mcp.json——本页是
 * 运行时管理面。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Cable, Pencil, Plus, RefreshCw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { toast } from "sonner";
import { api, type McpServerConfig, type McpServerStatus } from "@/api/client";
import { usePolling } from "@/lib/use-polling";

/** 键值对编辑行（env / headers 共用） */
interface KvRow {
  key: string;
  value: string;
}

const EMPTY_FORM: KvRow[] = [];

export function McpSettings() {
  const { t } = useTranslation();
  const [servers, setServers] = useState<McpServerStatus[]>([]);
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState("");
  const [transport, setTransport] = useState<"stdio" | "http">("stdio");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [url, setUrl] = useState("");
  const [toolTimeoutMs, setToolTimeoutMs] = useState("");
  const [env, setEnv] = useState<KvRow[]>(EMPTY_FORM);
  const [headers, setHeaders] = useState<KvRow[]>(EMPTY_FORM);
  /** 作用域（设置架构 §八）："" = 公共（所有 APP）；chat/coding = 仅该 APP */
  const [scope, setScope] = useState("");
  /** 编辑中的 server 名（null = 添加模式；编辑时名称锁定） */
  const [editing, setEditing] = useState<string | null>(null);

  const load = async () => {
    try {
      setServers(await api.mcpServers());
    } catch (err) {
      toast.error(t("settings.mcp.loadFailed", { error: String(err) }));
    }
  };

  useEffect(() => {
    void load();
  }, []);
  usePolling(() => void load(), 5000, true);

  /** 键值行 → Record（忽略空键行） */
  const kvToMap = (rows: KvRow[]): Record<string, string> => {
    const out: Record<string, string> = {};
    for (const row of rows) {
      const key = row.key.trim();
      if (key) out[key] = row.value;
    }
    return out;
  };

  const mapToKv = (map?: Record<string, string>): KvRow[] =>
    map ? Object.entries(map).map(([key, value]) => ({ key, value })) : [];

  const resetForm = () => {
    setName("");
    setTransport("stdio");
    setCommand("");
    setArgs("");
    setUrl("");
    setToolTimeoutMs("");
    setEnv(EMPTY_FORM);
    setHeaders(EMPTY_FORM);
    setScope("");
    setEditing(null);
  };

  /** 编辑已有 server：从 config.toml 配置回填表单（名称锁定） */
  const startEdit = async (serverName: string) => {
    try {
      const configs = await api.mcpConfigs();
      const cfg = configs.find((c) => c.name === serverName);
      if (!cfg) {
        toast.error(t("settings.mcp.configNotFound", { name: serverName }));
        return;
      }
      setEditing(serverName);
      setName(cfg.name);
      setTransport(cfg.transport);
      setCommand(cfg.command ?? "");
      setArgs((cfg.args ?? []).join(" "));
      setUrl(cfg.url ?? "");
      setToolTimeoutMs(cfg.tool_timeout_ms ? String(cfg.tool_timeout_ms) : "");
      setEnv(mapToKv(cfg.env));
      setHeaders(mapToKv(cfg.headers));
      const norm = (cfg.scopes ?? []).filter((s) => s && s !== "*");
      setScope(norm.length === 0 ? "" : norm[0]);
    } catch (err) {
      toast.error(t("settings.mcp.loadFailed", { error: String(err) }));
    }
  };

  const save = async () => {
    const cfg: McpServerConfig = {
      name: name.trim(),
      transport,
      ...(transport === "stdio"
        ? { command: command.trim(), args: args.split(/\s+/).filter(Boolean) }
        : { url: url.trim() }),
      ...(toolTimeoutMs.trim() ? { tool_timeout_ms: Number(toolTimeoutMs) } : {}),
    };
    const envMap = kvToMap(env);
    const headerMap = kvToMap(headers);
    if (Object.keys(envMap).length > 0) cfg.env = envMap;
    if (Object.keys(headerMap).length > 0) cfg.headers = headerMap;
    if (scope) cfg.scopes = [scope];
    if (!cfg.name || (transport === "stdio" ? !cfg.command : !cfg.url)) {
      toast.error(t("settings.mcp.formIncomplete"));
      return;
    }
    setBusy(true);
    try {
      await api.mcpConnect(cfg);
      toast.success(
        editing
          ? t("settings.mcp.updated", { name: cfg.name })
          : t("settings.mcp.connected", { name: cfg.name }),
      );
      resetForm();
      await load();
    } catch (err) {
      toast.error(t("settings.mcp.connectFailed", { error: String(err) }));
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async (serverName: string) => {
    try {
      await api.mcpDisconnect(serverName);
      toast.success(t("settings.mcp.disconnected", { name: serverName }));
      await load();
    } catch (err) {
      toast.error(t("settings.mcp.disconnectFailed", { error: String(err) }));
    }
  };

  /** 键值编辑器（env/headers 共用）：行列表 + 添加按钮 */
  const KvEditor = ({
    rows,
    onChange,
    keyPlaceholder,
    valuePlaceholder,
    addLabel,
  }: {
    rows: KvRow[];
    onChange: (rows: KvRow[]) => void;
    keyPlaceholder: string;
    valuePlaceholder: string;
    addLabel: string;
  }) => (
    <div className="space-y-1.5">
      {rows.map((row, i) => (
        <div key={i} className="flex items-center gap-1.5">
          <Input
            className="h-8 w-40 font-mono text-xs"
            placeholder={keyPlaceholder}
            value={row.key}
            onChange={(e) =>
              onChange(rows.map((r, j) => (j === i ? { ...r, key: e.target.value } : r)))
            }
          />
          <Input
            className="h-8 flex-1 font-mono text-xs"
            placeholder={valuePlaceholder}
            value={row.value}
            onChange={(e) =>
              onChange(rows.map((r, j) => (j === i ? { ...r, value: e.target.value } : r)))
            }
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-8 w-8 shrink-0"
            onClick={() => onChange(rows.filter((_, j) => j !== i))}
            aria-label={t("settings.mcp.removeRow")}
          >
            <Trash2 size={13} />
          </Button>
        </div>
      ))}
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() => onChange([...rows, { key: "", value: "" }])}
      >
        <Plus size={13} />
        {addLabel}
      </Button>
    </div>
  );

  return (
    <div className="space-y-5">
      <div>
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          <Cable size={18} />
          {t("settings.mcp.title")}
        </h2>
        <p className="mt-1 text-sm text-muted-foreground">
          {t("settings.mcp.desc")}
        </p>
      </div>

      {/* 已连接 server 列表 */}
      <div className="space-y-2">
        {servers.length === 0 && (
          <p className="text-sm text-muted-foreground">{t("settings.mcp.empty")}</p>
        )}
        {servers.map((s) => (
          <div
            key={s.name}
            className="flex items-center justify-between rounded-md border p-3"
          >
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <span className="font-medium">{s.name}</span>
                <Badge variant="secondary" className="text-[10px]">
                  {s.transport}
                </Badge>
                <Badge variant="outline" className="text-[10px] font-mono">
                  {s.protocol_version}
                </Badge>
              </div>
              <p className="mt-0.5 text-xs text-muted-foreground">
                {t("settings.mcp.toolCount", { count: s.tool_count })}
              </p>
            </div>
            <div className="flex shrink-0 items-center gap-1">
              <Button variant="ghost" size="sm" onClick={() => void startEdit(s.name)}>
                <Pencil size={14} />
                {t("settings.mcp.edit")}
              </Button>
              <Button variant="ghost" size="sm" onClick={() => disconnect(s.name)}>
                <Trash2 size={14} />
                {t("settings.mcp.disconnect")}
              </Button>
            </div>
          </div>
        ))}
      </div>

      {/* 添加 / 编辑 server 表单 */}
      <div className="rounded-md border p-3">
        <p className="mb-2 text-sm font-medium">
          {editing
            ? t("settings.mcp.editTitle", { name: editing })
            : t("settings.mcp.addTitle")}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <Input
            className="w-40"
            placeholder={t("settings.mcp.name")}
            value={name}
            disabled={!!editing}
            onChange={(e) => setName(e.target.value)}
          />
          <select
            className="h-9 rounded-md border bg-background px-2 text-sm"
            value={transport}
            onChange={(e) => setTransport(e.target.value as "stdio" | "http")}
          >
            <option value="stdio">stdio</option>
            <option value="http">http</option>
          </select>
          {transport === "stdio" ? (
            <>
              <Input
                className="w-44"
                placeholder={t("settings.mcp.command")}
                value={command}
                onChange={(e) => setCommand(e.target.value)}
              />
              <Input
                className="w-64"
                placeholder={t("settings.mcp.args")}
                value={args}
                onChange={(e) => setArgs(e.target.value)}
              />
            </>
          ) : (
            <Input
              className="w-72"
              placeholder="https://example.com/mcp"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
            />
          )}
          <Input
            className="w-32"
            placeholder={t("settings.mcp.timeout")}
            value={toolTimeoutMs}
            onChange={(e) => setToolTimeoutMs(e.target.value)}
          />
          <select
            className="h-9 rounded-md border bg-background px-2 text-sm"
            value={scope}
            onChange={(e) => setScope(e.target.value)}
            title={t("settings.mcp.scope")}
          >
            <option value="">{t("settings.mcp.scopePublic")}</option>
            <option value="chat">{t("settings.mcp.scopeChat")}</option>
            <option value="coding">{t("settings.mcp.scopeCoding")}</option>
          </select>
          {editing && (
            <Button size="sm" variant="ghost" onClick={resetForm}>
              {t("settings.mcp.cancel")}
            </Button>
          )}
          <Button size="sm" onClick={() => void save()} disabled={busy}>
            <Plus size={14} />
            {editing ? t("settings.mcp.save") : t("settings.mcp.connect")}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => void load()}>
            <RefreshCw size={14} />
          </Button>
        </div>

        {/* 环境变量（KEY 等敏感配置的载体）与 HTTP 头 */}
        <div className="mt-3 grid gap-3 md:grid-cols-2">
          <div>
            <p className="mb-1.5 text-xs font-medium text-muted-foreground">
              {t("settings.mcp.env")}
            </p>
            <KvEditor
              rows={env}
              onChange={setEnv}
              keyPlaceholder={t("settings.mcp.envKey")}
              valuePlaceholder={t("settings.mcp.envValue")}
              addLabel={t("settings.mcp.addEnv")}
            />
          </div>
          <div>
            <p className="mb-1.5 text-xs font-medium text-muted-foreground">
              {t("settings.mcp.headers")}
            </p>
            <KvEditor
              rows={headers}
              onChange={setHeaders}
              keyPlaceholder={t("settings.mcp.envKey")}
              valuePlaceholder={t("settings.mcp.envValue")}
              addLabel={t("settings.mcp.addHeader")}
            />
          </div>
        </div>
      </div>

      <p className="text-xs text-muted-foreground">{t("settings.mcp.hint")}</p>
    </div>
  );
}
