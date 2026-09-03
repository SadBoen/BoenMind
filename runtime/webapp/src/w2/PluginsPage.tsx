// 统一插件中心：整合「系统内置能力」与「外部 MCP 插件」
// 采用表格式呈现，顶部提供【全部 / 内置 / 外部】快速筛选，保留完整的扫描、配置与操作能力。
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Loader2Icon,
  PlusIcon,
  RefreshCwIcon,
  ScanSearchIcon,
  ShieldCheck,
  Globe,
} from "lucide-react";
import {
  api,
  type Capability,
  type McpListResult,
  type McpServer,
} from "./api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

type McpManifestSchemaItem = {
  key: string;
  label: string;
  hint?: string;
  type: "string" | "secret" | "range" | "select";
  default?: string | number;
  min?: number;
  max?: number;
  unit?: string;
  options?: { value: string; label: string }[];
};

type ConfigTarget = {
  name: string;
  schema: McpManifestSchemaItem[];
  values: Record<string, unknown>;
};

type McpCandidatesResult = {
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

type Draft = {
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

const emptyDraft: Draft = {
  name: "",
  transport: "stdio",
  url: "",
  bearer_token: "",
  command: "",
  args: "",
  env: "",
  tool_timeout_ms: "",
};

function toDraft(s: McpServer): Draft {
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

function fromDraft(d: Draft): Partial<McpServer> {
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

export type TablePluginItem = {
  id: string;
  name: string;
  type: "builtin" | "external";
  detail: string;
  statusText?: string;
  isOnline?: boolean;
  serverRef?: McpServer;
};

// 内置能力白话说明(键=能力名;未命中回落 effect 文案)
const BUILTIN_DESC: Record<string, string> = {
  "model.invoke": "内核私有 · 模型调用通道(每次回复都走它,非对话工具)",
  "system.exec": "系统终端:审批后执行命令(万能底牌)",
  "fs.search": "工作区内容搜索(rg 引擎内嵌)· 免审批直通",
  "fs.read": "读文件(带行号 + 分页)· 免审批直通",
  "fs.write": "写文件(新建/整文覆盖)· 需审批",
  "fs.edit": "精确字符串替换编辑 · 需审批",
};

export function PluginsPage({
  initialFilter,
  editTarget,
  onConsumedEditTarget,
}: {
  initialFilter?: string;
  editTarget?: string | null;
  onConsumedEditTarget?: () => void;
}) {
  const [filter, setFilter] = useState(initialFilter ?? "");
  const [typeFilter, setTypeFilter] = useState<"all" | "builtin" | "external">("all");
  const [builtinList, setBuiltinList] = useState<Capability[]>([]);
  const [mcpData, setMcpData] = useState<McpListResult | null>(null);
  const [statusMap, setStatusMap] = useState<Record<string, { ok: boolean; tools?: number; error?: string }>>({});
  
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [reloading, setReloading] = useState(false);
  const [scanResult, setScanResult] = useState<McpCandidatesResult | null>(null);
  const [approving, setApproving] = useState<string | null>(null);
  const [configTarget, setConfigTarget] = useState<ConfigTarget | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const loadData = useCallback(async () => {
    try {
      const [bRes, mRes] = await Promise.all([
        api.capabilities().catch(() => ({ builtin: [] })),
        api.mcp.list().catch(() => null),
      ]);
      setBuiltinList(bRes.builtin ?? []);
      setMcpData(mRes);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      // /admin/mcp/status 返回形如 { status: [{name, ok, tools, error}] }
      const s = await api.mcp.status();
      const map: Record<string, { ok: boolean; tools?: number; error?: string }> = {};
      for (const item of s.status ?? []) {
        map[item.name] = { ok: item.ok, tools: item.tools, error: item.error };
      }
      setStatusMap(map);
    } catch {
      // 忽略探活静默错误
    }
  }, []);

  useEffect(() => {
    void loadData();
    void refreshStatus();
    const timer = setInterval(() => {
      void refreshStatus();
    }, 30000);
    return () => clearInterval(timer);
  }, [loadData, refreshStatus]);

  useEffect(() => {
    if (editTarget && mcpData?.servers) {
      const found = mcpData.servers.find((s) => s.name === editTarget);
      if (found) {
        setDraft(toDraft(found));
        onConsumedEditTarget?.();
      }
    }
  }, [editTarget, mcpData, onConsumedEditTarget]);

  // 统一列表聚合
  const tableItems: TablePluginItem[] = useMemo(() => {
    const list: TablePluginItem[] = [];

    // 1. 系统内置能力
    for (const b of builtinList) {
      const effectText =
        b.effect === "read-only"
          ? "只读直通"
          : b.effect != null && b.effect !== "read-only"
            ? "需审批"
            : "系统基础能力";
      list.push({
        id: `builtin:${b.name}`,
        name: b.name,
        type: "builtin",
        detail:
          BUILTIN_DESC[b.name] ??
          `${effectText}${b.idempotent ? " · 幂等" : ""}`,
        statusText: "就绪",
        isOnline: true,
      });
    }

    // 2. 外部 MCP 插件
    if (mcpData?.servers) {
      for (const s of mcpData.servers) {
        const st = statusMap[s.name];
        const isOk = st?.ok ?? false;
        list.push({
          id: `mcp:${s.name}`,
          name: s.name,
          type: "external",
          detail:
            s.transport === "stdio" && s.args?.length
              ? `参数: ${s.args.join(" ")}`
              : "外部扩展服务",
          statusText: isOk ? `联通 (${st?.tools ?? 0} 工具)` : "离线 / 未装载",
          isOnline: isOk,
          serverRef: s,
        });
      }
    }

    return list;
  }, [builtinList, mcpData, statusMap]);

  // 快速筛选与关键字搜索
  const filteredItems = useMemo(() => {
    return tableItems.filter((item) => {
      if (typeFilter !== "all" && item.type !== typeFilter) return false;
      if (filter.trim()) {
        const kw = filter.trim().toLowerCase();
        return item.name.toLowerCase().includes(kw) || item.detail.toLowerCase().includes(kw);
      }
      return true;
    });
  }, [tableItems, typeFilter, filter]);

  const scanPlugins = async () => {
    setScanning(true);
    setError(null);
    try {
      setScanResult(await api.mcp.candidates());
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setScanning(false);
    }
  };

  const handleReload = async () => {
    setReloading(true);
    try {
      const r = await api.mcp.reload();
      const parts: string[] = [];
      if (r.registered?.length) parts.push("新增: " + r.registered.join("、"));
      if (r.updated?.length) parts.push("更新: " + r.updated.join("、"));
      if (r.uninstalled?.length) parts.push("卸载: " + r.uninstalled.join("、"));
      setNotice(parts.length ? parts.join("; ") : (r.note ?? "无变更"));
      await loadData();
      await refreshStatus();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setReloading(false);
    }
  };

  const handleRemove = async (name: string) => {
    if (!confirm(`确定移除 MCP 服务「${name}」？此操作将从配置中删除。`)) return;
    setBusy(true);
    try {
      await api.mcp.remove(name);
      await loadData();
      await refreshStatus();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  const handleTest = async (name: string) => {
    try {
      const r = await api.mcp.test(name);
      setStatusMap((prev) => ({
        ...prev,
        [name]: { ok: r.ok, tools: r.tools?.length, error: r.error },
      }));
      if (!r.ok) {
        alert(`测试「${name}」未通过: ${r.error || "服务无响应"}`);
      }
    } catch (e) {
      alert(`测试「${name}」失败: ${e}`);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      {/* 顶部标题与操作栏 */}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-[16px] font-semibold text-foreground">插件与能力中心</h2>
          <p className="text-muted-foreground text-[12.5px] mt-0.5">
            查看系统内置基础手脚，管理外部扩展插件（MCP 工具、真实 App）与连通状态。
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={scanning}
            className="h-8 text-[12px] gap-1.5"
            onClick={() => void scanPlugins()}
          >
            {scanning ? <Loader2Icon className="size-3.5 animate-spin" /> : <ScanSearchIcon className="size-3.5" />}
            扫描候选
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={reloading}
            className="h-8 text-[12px] gap-1.5"
            onClick={() => void handleReload()}
          >
            {reloading ? <Loader2Icon className="size-3.5 animate-spin" /> : <RefreshCwIcon className="size-3.5" />}
            热重载
          </Button>
          <Button
            size="sm"
            className="h-8 text-[12px] gap-1"
            onClick={() => setDraft({ ...emptyDraft })}
          >
            <PlusIcon className="size-3.5" /> 接入外部插件
          </Button>
        </div>
      </div>

      {/* 过滤栏：搜索框 + 快速筛选按钮 */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-b pb-3">
        <Input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="搜索插件或能力名称…"
          className="h-8.5 max-w-xs text-[12.5px]"
        />

        <div className="flex items-center gap-1 rounded-lg border bg-muted/30 p-1">
          <Button
            size="sm"
            variant={typeFilter === "all" ? "default" : "ghost"}
            className="h-6.5 px-2.5 text-[11.5px]"
            onClick={() => setTypeFilter("all")}
          >
            全部 ({tableItems.length})
          </Button>
          <Button
            size="sm"
            variant={typeFilter === "builtin" ? "default" : "ghost"}
            className="h-6.5 px-2.5 text-[11.5px]"
            onClick={() => setTypeFilter("builtin")}
          >
            内置 ({builtinList.length})
          </Button>
          <Button
            size="sm"
            variant={typeFilter === "external" ? "default" : "ghost"}
            className="h-6.5 px-2.5 text-[11.5px]"
            onClick={() => setTypeFilter("external")}
          >
            外部 ({mcpData?.servers?.length ?? 0})
          </Button>
        </div>
      </div>

      {notice ? (
        <div className="rounded-lg bg-emerald-500/10 px-3 py-2 text-[12.5px] text-emerald-600">
          {notice}
        </div>
      ) : null}
      {error ? (
        <div className="rounded-lg bg-destructive/10 px-3 py-2 text-[12.5px] text-destructive">
          {error}
        </div>
      ) : null}

      {/* 核心展示区：表格式 (Table) 设计 */}
      <div className="overflow-hidden rounded-xl border bg-card/60 shadow-xs backdrop-blur-sm">
        <table className="w-full text-left text-[12.5px] table-fixed">
          <thead className="border-b bg-muted/40 text-[11.5px] font-semibold text-muted-foreground uppercase">
            <tr>
              <th className="px-3.5 py-2.5">名称 / 标识</th>
              <th className="px-3 py-2.5 w-24 whitespace-nowrap">类别</th>
              <th className="px-3 py-2.5 w-36 whitespace-nowrap">状态</th>
              <th className="px-3.5 py-2.5 w-56 whitespace-nowrap">操作</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border/60">
            {filteredItems.map((item) => {
              const mcpEntry = mcpData?.entries?.find((e) => e.server.name === item.name);
              return (
                <tr key={item.id} className="transition-colors hover:bg-muted/30">
                  {/* 名称与描述 */}
                  <td className="px-3.5 py-2.5 align-middle">
                    <div className="flex items-center gap-2">
                      <span
                        className={cn(
                          "size-2 shrink-0 rounded-full",
                          item.isOnline ? "bg-emerald-500" : "bg-zinc-400",
                        )}
                        title={item.isOnline ? "可用" : "未联通"}
                      />
                      <span className="font-mono font-medium text-foreground">{item.name}</span>
                    </div>
                    <div className="text-muted-foreground mt-0.5 max-w-xs truncate text-[11.5px]">
                      {item.detail}
                    </div>
                  </td>

                  {/* 类别徽标 */}
                  <td className="px-3 py-2.5 align-middle whitespace-nowrap">
                    {item.type === "builtin" ? (
                      <Badge variant="outline" className="gap-1 border-blue-500/30 bg-blue-500/10 font-mono text-[10.5px] text-blue-600 dark:text-blue-400">
                        <ShieldCheck className="size-3" /> 系统内置
                      </Badge>
                    ) : (
                      <Badge variant="secondary" className="gap-1 font-mono text-[10.5px]">
                        <Globe className="size-3 opacity-70" /> 外部 MCP
                      </Badge>
                    )}
                  </td>

                  {/* 状态 */}
                  <td className="px-3 py-2.5 align-middle whitespace-nowrap">
                    <span
                      className={cn(
                        "inline-block max-w-full overflow-hidden text-ellipsis align-middle text-[12px] font-medium",
                        item.isOnline ? "text-emerald-600 dark:text-emerald-400" : "text-muted-foreground",
                      )}
                    >
                      {item.statusText}
                    </span>
                  </td>

                  {/* 操作按钮组 */}
                  <td className="px-3.5 py-2.5 text-right align-middle whitespace-nowrap">
                    {item.type === "external" && item.serverRef ? (
                      <div className="flex items-center justify-end gap-0.5">
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-7 px-2 text-[11.5px]"
                          onClick={() => setDraft(toDraft(item.serverRef!))}
                        >
                          编辑
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-7 px-2 text-[11.5px]"
                          onClick={() => void handleTest(item.name)}
                          title="主动探活 (tools/list)"
                        >
                          测试
                        </Button>
                        {mcpEntry?.manifest?.config_schema?.length ? (
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-7 px-2 text-[11.5px]"
                            onClick={() => {
                              api.mcp.getConfig(item.name).then((cfg) => {
                                setConfigTarget({
                                  name: item.name,
                                  schema: mcpEntry.manifest?.config_schema ?? [],
                                  values: cfg.values,
                                });
                              });
                            }}
                          >
                            参数
                          </Button>
                        ) : null}
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={busy}
                          className="h-7 px-2 text-[11.5px] text-destructive hover:bg-destructive/10"
                          onClick={() => void handleRemove(item.name)}
                        >
                          移除
                        </Button>
                      </div>
                    ) : (
                      <span className="text-[11px] text-muted-foreground/60 select-none">
                        出厂固有 · 禁卸载
                      </span>
                    )}
                  </td>
                </tr>
              );
            })}

            {filteredItems.length === 0 ? (
              <tr>
                <td colSpan={4} className="py-8 text-center text-muted-foreground text-[12.5px]">
                  没有找到匹配的插件或能力条目。
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </div>

      {/* 参数配置抽屉对话框 */}
      <ServerConfigDialog
        target={configTarget}
        onClose={() => setConfigTarget(null)}
      />

      {/* 插件扫描发现对话框 */}
      {scanResult ? (
        <Dialog open onOpenChange={(v) => !v && setScanResult(null)}>
          <DialogContent className="sm:max-w-lg">
            <DialogHeader>
              <DialogTitle>插件目录扫描</DialogTitle>
              <DialogDescription>
                扫描路径: {scanResult.dir}
                {scanResult.bundled_dir ? ` · 随包目录: ${scanResult.bundled_dir}` : ""}
              </DialogDescription>
            </DialogHeader>
            {scanResult.candidates.length ? (
              <div className="max-h-72 space-y-2 overflow-auto">
                {scanResult.candidates.map((c) => (
                  <div
                    key={c.name}
                    className="flex items-start justify-between gap-3 rounded-lg border p-2.5"
                  >
                    <div className="min-w-0">
                      <div className="text-sm font-medium">
                        {c.title || c.name}{" "}
                        {c.source === "bundled" ? (
                          <span className="text-muted-foreground text-xs">(官方随包)</span>
                        ) : null}
                        {c.registered ? (
                          <span className="text-emerald-600 text-xs ml-1">(已登记)</span>
                        ) : null}
                      </div>
                      <div className="text-muted-foreground truncate text-xs mt-0.5">
                        {c.description || c.file}
                      </div>
                    </div>
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={c.registered || approving === c.name}
                      onClick={async () => {
                        setApproving(c.name);
                        try {
                          await api.mcp.approve(c.name);
                          setScanResult(null);
                          await api.mcp.reload();
                          await loadData();
                          await refreshStatus();
                        } catch (e) {
                          setError(String(e instanceof Error ? e.message : e));
                        } finally {
                          setApproving(null);
                        }
                      }}
                    >
                      {c.registered ? "已批准" : "批准接入"}
                    </Button>
                  </div>
                ))}
              </div>
            ) : (
              <div className="text-muted-foreground py-6 text-center text-[12.5px]">
                未在扫描路径中发现新的插件可执行文件。
              </div>
            )}
            <DialogFooter>
              <Button variant="outline" onClick={() => setScanResult(null)}>
                关闭
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      ) : null}

      {/* 新增/编辑外部插件表单 */}
      {draft ? (
        <McpDialog
          draft={draft}
          busy={busy}
          onClose={() => setDraft(null)}
          onSave={async (d) => {
            setBusy(true);
            try {
              if (d._editing) {
                await api.mcp.update(d.name, fromDraft(d));
              } else {
                await api.mcp.add(fromDraft(d));
              }
              setDraft(null);
              await loadData();
              await refreshStatus();
            } catch (e) {
              setError(String(e instanceof Error ? e.message : e));
            } finally {
              setBusy(false);
            }
          }}
        />
      ) : null}
    </div>
  );
}

// 参数配置抽屉
function ServerConfigDialog({
  target,
  onClose,
}: {
  target: ConfigTarget | null;
  onClose: () => void;
}) {
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (target) {
      setValues(target.values || {});
      setErr(null);
    }
  }, [target]);

  if (!target) return null;

  const handleSave = async () => {
    setSaving(true);
    setErr(null);
    try {
      await api.mcp.setConfig(target.name, values);
      onClose();
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>配置参数 · {target.name}</DialogTitle>
          <DialogDescription>
            该插件声明了可调节的运行配置参数。
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3 py-2">
          {target.schema.map((item) => (
            <div key={item.key} className="space-y-1">
              <Label className="text-xs font-medium">
                {item.label || item.key}
                {item.hint ? (
                  <span className="text-muted-foreground ml-1 font-normal">
                    ({item.hint})
                  </span>
                ) : null}
              </Label>
              <Input
                value={String(values[item.key] ?? item.default ?? "")}
                onChange={(e) =>
                  setValues((prev) => ({ ...prev, [item.key]: e.target.value }))
                }
                className="h-8 font-mono text-xs"
              />
            </div>
          ))}
          {err ? <div className="text-destructive text-xs">{err}</div> : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={saving}>
            取消
          </Button>
          <Button onClick={() => void handleSave()} disabled={saving}>
            {saving ? <Loader2Icon className="animate-spin" /> : null} 保存参数
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// 外部插件编辑/新增对话框
function McpDialog({
  draft,
  busy,
  onClose,
  onSave,
}: {
  draft: Draft;
  busy: boolean;
  onClose: () => void;
  onSave: (d: Draft) => Promise<void>;
}) {
  const [form, setForm] = useState<Draft>(draft);
  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{form._editing ? "编辑插件" : "接入外部插件"}</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-3 py-2 text-[13px]">
          <div className="flex flex-col gap-1">
            <Label className="text-xs">插件名称</Label>
            <Input
              disabled={form._editing}
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="唯一标识，如 web-multisearch"
              className="h-8 font-mono text-xs"
            />
          </div>

          <div className="flex flex-col gap-1">
            <Label className="text-xs">通信协议 (Transport)</Label>
            <select
              value={form.transport}
              onChange={(e) =>
                setForm({
                  ...form,
                  transport: e.target.value as Draft["transport"],
                })
              }
              className="border-input bg-background h-8 rounded-md border px-2 font-mono text-[12px] outline-none"
            >
              <option value="stdio">stdio (本地子进程)</option>
              <option value="sse">sse (远程流式 / Server-Sent Events)</option>
              <option value="http">http (远程 HTTP)</option>
              <option value="streamable-http">streamable-http (远程 Streamable HTTP)</option>
            </select>
          </div>

          {form.transport === "stdio" ? (
            <>
              <div className="flex flex-col gap-1">
                <Label className="text-xs">命令路径 (Command)</Label>
                <Input
                  value={form.command}
                  onChange={(e) => setForm({ ...form, command: e.target.value })}
                  placeholder="可执行文件完整路径"
                  className="h-8 font-mono text-xs"
                />
              </div>
              <div className="flex flex-col gap-1">
                <Label className="text-xs">命令行参数 (空格分隔)</Label>
                <Input
                  value={form.args}
                  onChange={(e) => setForm({ ...form, args: e.target.value })}
                  placeholder="如: --config /path/to/cfg"
                  className="h-8 font-mono text-xs"
                />
              </div>
            </>
          ) : (
            <>
              <div className="flex flex-col gap-1">
                <Label className="text-xs">远程服务 URL</Label>
                <Input
                  value={form.url}
                  onChange={(e) => setForm({ ...form, url: e.target.value })}
                  placeholder="https://example.com/mcp"
                  className="h-8 font-mono text-xs"
                />
              </div>
              <div className="flex flex-col gap-1">
                <Label className="text-xs">Bearer Token (可选)</Label>
                <Input
                  type="password"
                  value={form.bearer_token}
                  onChange={(e) => setForm({ ...form, bearer_token: e.target.value })}
                  className="h-8 font-mono text-xs"
                />
              </div>
            </>
          )}

          <div className="flex flex-col gap-1">
            <Label className="text-xs">环境变量 (KEY=VAL 每行一条)</Label>
            <textarea
              rows={2}
              value={form.env}
              onChange={(e) => setForm({ ...form, env: e.target.value })}
              className="border-input bg-background rounded-md border p-2 font-mono text-xs outline-none resize-none"
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button onClick={() => void onSave(form)} disabled={busy || !form.name.trim()}>
            {busy ? <Loader2Icon className="animate-spin" /> : null} 保存
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
