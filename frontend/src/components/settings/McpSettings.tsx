/**
 * MCP 设置页（bm-mcp 官方插件）：外部 MCP server 管理——
 * 已连接 server 状态（协议版本/工具数）+ 运行时连接/断开（即时生效
 * 并持久化到 config.toml）+ 标准配置自动发现提示。
 *
 * 配置也可直接放 config.toml 的 `mcp` 数组，或项目 `.mcp.json`
 * （Claude Code 格式）/ ~/.agents/mcp.json——本页是运行时管理面。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Cable, Plus, RefreshCw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { toast } from "sonner";
import { api, type McpServerConfig, type McpServerStatus } from "@/api/client";

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

  const load = async () => {
    try {
      setServers(await api.mcpServers());
    } catch (err) {
      toast.error(t("settings.mcp.loadFailed", { error: String(err) }));
    }
  };

  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(), 5000);
    return () => clearInterval(timer);
  }, []);

  const connect = async () => {
    const cfg: McpServerConfig = {
      name: name.trim(),
      transport,
      ...(transport === "stdio" ? { command: command.trim(), args: args.split(/\s+/).filter(Boolean) } : { url: url.trim() }),
      ...(toolTimeoutMs.trim() ? { tool_timeout_ms: Number(toolTimeoutMs) } : {}),
    };
    if (!cfg.name || (transport === "stdio" ? !cfg.command : !cfg.url)) {
      toast.error(t("settings.mcp.formIncomplete"));
      return;
    }
    setBusy(true);
    try {
      await api.mcpConnect(cfg);
      toast.success(t("settings.mcp.connected", { name: cfg.name }));
      setName("");
      setCommand("");
      setArgs("");
      setUrl("");
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
            <Button variant="ghost" size="sm" onClick={() => disconnect(s.name)}>
              <Trash2 size={14} />
              {t("settings.mcp.disconnect")}
            </Button>
          </div>
        ))}
      </div>

      {/* 添加 server 表单 */}
      <div className="rounded-md border p-3">
        <p className="mb-2 text-sm font-medium">{t("settings.mcp.addTitle")}</p>
        <div className="flex flex-wrap items-center gap-2">
          <Input
            className="w-40"
            placeholder={t("settings.mcp.name")}
            value={name}
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
          <Button size="sm" onClick={connect} disabled={busy}>
            <Plus size={14} />
            {t("settings.mcp.connect")}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => void load()}>
            <RefreshCw size={14} />
          </Button>
        </div>
      </div>

      <p className="text-xs text-muted-foreground">{t("settings.mcp.hint")}</p>
    </div>
  );
}
