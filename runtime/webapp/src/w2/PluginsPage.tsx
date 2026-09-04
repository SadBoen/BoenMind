// 统一插件中心：整合「系统内置能力」与「外部 MCP 插件」
// 采用表格式呈现，顶部提供【全部 / 内置 / 外部】快速筛选，保留完整的扫描、配置与操作能力。
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Loader2Icon,
  PlusIcon,
  RefreshCwIcon,
  ScanSearchIcon,
  ShieldCheck,
  Globe,
  Wrench,
  PlayIcon,
  XIcon,
} from "lucide-react";
import {
  api,
  type Capability,
  type McpListResult,
  type McpServer,
  type ProviderManifestItem,
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { STORAGE_KEYS, storage } from "@/lib/storage";
import { Switch } from "@/components/ui/switch";
import { Tooltip } from "radix-ui";

type McpManifestSchemaItem = {
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

type ProviderEntry = ProviderManifestItem & {
  /** 是否已在「已配置」列表中(用于下拉区分) */
  present?: boolean;
};

type ConfigTarget = {
  name: string;
  server?: McpServer;
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

export type ToolInfo = {
  name: string;
  description?: string;
};

export type TablePluginItem = {
  id: string;
  name: string;
  type: "builtin" | "external";
  detail: string;
  tools: ToolInfo[];
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

// 表格列定义与默认列宽(按用户反馈优化:名称标识适度收紧,类别与状态拉开,操作居中)
type ColKey = "name" | "category" | "tools" | "actions";

interface ColConfig {
  key: ColKey;
  label: string;
  defaultWidth: number;
  minWidth: number;
  align?: "left" | "center" | "right";
}

const TABLE_COLUMNS: ColConfig[] = [
  { key: "name", label: "名称 / 标识", defaultWidth: 220, minWidth: 160, align: "left" },
  { key: "category", label: "类别", defaultWidth: 110, minWidth: 90, align: "left" },
  { key: "tools", label: "提供工具", defaultWidth: 260, minWidth: 180, align: "left" },
  { key: "actions", label: "操作", defaultWidth: 180, minWidth: 150, align: "center" },
];

function loadColWidths(): Record<ColKey, number> {
  const defaults: Record<ColKey, number> = {
    name: 220,
    category: 110,
    tools: 260,
    actions: 180,
  };
  const raw = storage.get(STORAGE_KEYS.PLUGINS_TABLE_COLS);
  if (!raw) return defaults;
  try {
    const parsed = JSON.parse(raw);
    return {
      name: typeof parsed.name === "number" ? Math.max(160, parsed.name) : defaults.name,
      category: typeof parsed.category === "number" ? Math.max(90, parsed.category) : defaults.category,
      tools: typeof parsed.tools === "number" ? Math.max(180, parsed.tools) : defaults.tools,
      actions: typeof parsed.actions === "number" ? Math.max(150, parsed.actions) : defaults.actions,
    };
  } catch {
    return defaults;
  }
}

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
  const [statusMap, setStatusMap] = useState<
    Record<string, { ok: boolean; tools?: number; tool_list?: ToolInfo[]; error?: string }>
  >({});
  
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [reloading, setReloading] = useState(false);
  const [scanResult, setScanResult] = useState<McpCandidatesResult | null>(null);
  const [approving, setApproving] = useState<string | null>(null);
  const [configTarget, setConfigTarget] = useState<ConfigTarget | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // 表格列宽状态(支持记忆与拖动)
  const [colWidths, setColWidths] = useState<Record<ColKey, number>>(loadColWidths);
  const colWidthsRef = useRef(colWidths);
  colWidthsRef.current = colWidths;

  const handleResizeStart = useCallback(
    (key: ColKey, e: React.PointerEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const startX = e.clientX;
      const startWidth = colWidthsRef.current[key];
      const col = TABLE_COLUMNS.find((c) => c.key === key);
      const minW = col?.minWidth ?? 80;

      const onPointerMove = (moveEvt: PointerEvent) => {
        const deltaX = moveEvt.clientX - startX;
        const newWidth = Math.max(minW, startWidth + deltaX);
        setColWidths((prev) => {
          const next = { ...prev, [key]: newWidth };
          colWidthsRef.current = next;
          return next;
        });
      };

      const onPointerUp = () => {
        window.removeEventListener("pointermove", onPointerMove);
        window.removeEventListener("pointerup", onPointerUp);
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
        // 持久化保存用户调节后的列宽
        storage.set(
          STORAGE_KEYS.PLUGINS_TABLE_COLS,
          JSON.stringify(colWidthsRef.current),
        );
      };

      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
      window.addEventListener("pointermove", onPointerMove);
      window.addEventListener("pointerup", onPointerUp);
    },
    [],
  );

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
      // /admin/mcp/status 返回形如 { status: [{name, ok, tools, tool_list, error}] }
      const s = await api.mcp.status();
      const map: Record<
        string,
        { ok: boolean; tools?: number; tool_list?: ToolInfo[]; error?: string }
      > = {};
      for (const item of s.status ?? []) {
        map[item.name] = {
          ok: item.ok,
          tools: item.tools,
          tool_list: item.tool_list,
          error: item.error,
        };
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
        tools: [
          {
            name: b.name,
            description: BUILTIN_DESC[b.name] ?? `${effectText}能力`,
          },
        ],
        isOnline: true,
      });
    }

    // 2. 官方透视插件(前端内建只读诊断面,主对话区「上下文」页签直达)
    list.push({
      id: "official:context-inspector",
      name: "context-inspector",
      type: "builtin",
      detail: "官方透视插件 · 交互透视与诊断分析(只读快照/窗口水位/细分分账/副作用追踪/双栏联动)",
      tools: [
        {
          name: "context.inspect",
          description: "对话上下文全域双栏透视与真实快照(主界面「上下文」页签)",
        },
        {
          name: "context.headroom",
          description: "模型真实上下文窗口与余量实时监控(依设置中心登记值计算)",
        },
        {
          name: "context.file_tracker",
          description: "本地工程文件读写与副作用追踪",
        },
        {
          name: "context.spike_diagnose",
          description: "多轮历史 Token 异常暴增诊断",
        },
      ],
      isOnline: true,
    });

    // 3. 外部 MCP 插件
    if (mcpData?.servers) {
      for (const s of mcpData.servers) {
        const st = statusMap[s.name];
        const isOk = st?.ok ?? false;
        // 如果后端探活返回了具体的 tool_list 则优先使用；否则若有 tools 数量做保底
        const tools: ToolInfo[] = (st?.tool_list && st.tool_list.length > 0)
          ? st.tool_list
          : (st?.tools ?? 0) > 0
            ? Array.from({ length: st!.tools! }, (_, i) => ({
                name: `tool_${i + 1}`,
                description: "外部 MCP 工具",
              }))
            : [];

        list.push({
          id: `mcp:${s.name}`,
          name: s.name,
          type: "external",
          detail:
            s.transport === "stdio" && s.args?.length
              ? `参数: ${s.args.join(" ")}`
              : "外部扩展服务",
          tools,
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
        [name]: { ok: r.ok, tools: r.tools, error: r.error },
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
            查看系统内置基础能力，管理外部扩展插件（MCP 工具、真实 App）与连通状态。
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

      {/* 过滤栏:搜索框 + 快速筛选按钮(表格卡片自带边框,不再加分隔线) */}
      <div className="flex flex-wrap items-center justify-between gap-3">
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
        <div className="notice-success">
          {notice}
        </div>
      ) : null}
      {error ? (
        <div className="notice-error">
          {error}
        </div>
      ) : null}

      {/* 核心展示区：表格式 (Table) 设计 (支持列间分隔线与拖拽调整列宽) */}
      <div className="overflow-x-auto rounded-xl border bg-card/60 shadow-xs backdrop-blur-sm">
        <table className="w-full text-left text-[12.5px] table-fixed border-collapse">
          <colgroup>
            <col style={{ width: `${colWidths.name}px` }} />
            <col style={{ width: `${colWidths.category}px` }} />
            <col style={{ width: `${colWidths.tools}px` }} />
            <col style={{ width: `${colWidths.actions}px` }} />
          </colgroup>
          <thead className="border-b bg-muted/40 text-[11.5px] font-semibold text-muted-foreground uppercase select-none">
            <tr>
              {TABLE_COLUMNS.map((col) => (
                <th
                  key={col.key}
                  className={cn(
                    "relative px-3.5 py-2.5 border-r border-border/50 last:border-r-0 whitespace-nowrap overflow-hidden text-ellipsis",
                    col.align === "center" ? "text-center" : col.align === "right" ? "text-right" : "text-left",
                  )}
                >
                  <span>{col.label}</span>
                  {/* 可拖拽列边框 */}
                  <div
                    className="col-resizer"
                    onPointerDown={(e) => handleResizeStart(col.key, e)}
                    title="按住左右拖动调整列宽"
                  />
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y divide-border/60">
            {filteredItems.map((item) => {
              const mcpEntry = mcpData?.entries?.find((e) => e.server.name === item.name);
              return (
                <tr key={item.id} className="transition-colors hover:bg-muted/30">
                  {/* 名称与描述 */}
                  <td className="px-3.5 py-2.5 align-middle border-r border-border/40 overflow-hidden">
                    <div className="flex items-center gap-2 min-w-0">
                      <span
                        className={cn(
                          "size-2 shrink-0 rounded-full",
                          item.isOnline ? "bg-emerald-500" : "bg-zinc-400",
                        )}
                        title={item.isOnline ? "可用" : "未联通"}
                      />
                      <span className="font-mono font-medium text-foreground truncate">{item.name}</span>
                    </div>
                    <div className="text-muted-foreground mt-0.5 truncate text-[11.5px]">
                      {item.detail}
                    </div>
                  </td>

                  {/* 类别徽标 */}
                  <td className="px-4 py-2.5 align-middle whitespace-nowrap border-r border-border/40">
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

                  {/* 提供工具列表(标签 + 气泡防撑破) */}
                  <td className="px-3.5 py-2.5 align-middle border-r border-border/40 overflow-hidden">
                    <Tooltip.Provider delayDuration={200}>
                      <div className="flex flex-wrap items-center gap-1.5 min-w-0">
                        {item.tools.slice(0, 2).map((t) => (
                          <Tooltip.Root key={t.name}>
                            <Tooltip.Trigger asChild>
                              <span
                                className={cn(
                                  "inline-flex items-center gap-1 px-2 py-0.5 rounded-md font-mono text-[11px] border max-w-[135px] truncate cursor-help transition-colors",
                                  item.type === "builtin"
                                    ? "bg-muted/50 border-border text-foreground hover:bg-muted"
                                    : "bg-emerald-500/10 border-emerald-500/30 text-emerald-600 dark:text-emerald-400 hover:bg-emerald-500/20",
                                )}
                              >
                                <Wrench className="size-2.5 shrink-0 opacity-70" />
                                <span className="truncate">{t.name}</span>
                              </span>
                            </Tooltip.Trigger>
                            <Tooltip.Portal>
                              <Tooltip.Content
                                side="top"
                                align="start"
                                sideOffset={5}
                                className="z-50 max-w-xs rounded-lg border bg-popover p-2.5 text-[11.5px] text-popover-foreground shadow-md animate-in fade-in-0 zoom-in-95"
                              >
                                <div className="font-mono font-semibold text-foreground flex items-center gap-1">
                                  <Wrench className="size-3 text-emerald-500" />
                                  {t.name}
                                </div>
                                {t.description ? (
                                  <div className="text-muted-foreground mt-1 text-[11px] leading-relaxed">
                                    {t.description}
                                  </div>
                                ) : (
                                  <div className="text-muted-foreground/60 mt-1 text-[10.5px]">
                                    暂无工具详细描述
                                  </div>
                                )}
                                <Tooltip.Arrow className="fill-popover" />
                              </Tooltip.Content>
                            </Tooltip.Portal>
                          </Tooltip.Root>
                        ))}

                        {/* 超出 2 个工具时显示折叠徽标，鼠标悬浮气泡查看全部 */}
                        {item.tools.length > 2 ? (
                          <Tooltip.Root>
                            <Tooltip.Trigger asChild>
                              <span className="inline-flex items-center px-1.5 py-0.5 rounded-md font-mono text-[10.5px] font-semibold bg-muted text-muted-foreground border border-border cursor-help hover:text-foreground">
                                +{item.tools.length - 2}
                              </span>
                            </Tooltip.Trigger>
                            <Tooltip.Portal>
                              <Tooltip.Content
                                side="top"
                                align="start"
                                sideOffset={5}
                                className="z-50 max-w-sm rounded-lg border bg-popover p-3 text-[11.5px] text-popover-foreground shadow-lg animate-in fade-in-0 zoom-in-95"
                              >
                                <div className="font-semibold text-foreground border-b pb-1.5 mb-2 flex items-center justify-between">
                                  <span>全部可用工具清单</span>
                                  <span className="text-[10.5px] font-mono text-muted-foreground">共 {item.tools.length} 个</span>
                                </div>
                                <div className="space-y-2 max-h-56 overflow-y-auto pr-1">
                                  {item.tools.map((t) => (
                                    <div key={t.name} className="rounded bg-muted/40 p-1.5 border border-border/50">
                                      <div className="font-mono font-medium text-foreground flex items-center gap-1">
                                        <Wrench className="size-3 text-emerald-500 shrink-0" />
                                        <span>{t.name}</span>
                                      </div>
                                      {t.description ? (
                                        <div className="text-muted-foreground mt-0.5 text-[10.5px] leading-relaxed">
                                          {t.description}
                                        </div>
                                      ) : null}
                                    </div>
                                  ))}
                                </div>
                                <Tooltip.Arrow className="fill-popover" />
                              </Tooltip.Content>
                            </Tooltip.Portal>
                          </Tooltip.Root>
                        ) : null}

                        {item.tools.length === 0 ? (
                          <span className="text-[11px] text-muted-foreground/60 italic">
                            未探测到可用工具
                          </span>
                        ) : null}
                      </div>
                    </Tooltip.Provider>
                  </td>

                  {/* 操作按钮组 (水平中间对齐) */}
                  <td className="px-3.5 py-2.5 text-center align-middle whitespace-nowrap">
                    {item.type === "external" && item.serverRef ? (
                      <div className="flex items-center justify-center gap-1">
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
                            配置
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
                      <span className="text-[11px] text-muted-foreground/60 select-none inline-block">
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

      {/* 配置抽屉对话框 */}
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
                await api.mcp.create(fromDraft(d));
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

// 配置抽屉(2026-09-04 重写:支持 web_multisearch 的 providers 型配置
// ——下拉式供应商列表 + 新增行 + 每家可编辑字段 + 左右分栏用量进度条 + 真搜索测试)
//
// 布局:
//   概览(顶部标题)
//   左: 用量进度条列表      右: 供应商下拉(顶行「新增 + 」)+ 选中家表单 + 测试按钮
//   底部: 常规字段(default_limit)+ 保存/取消
function ServerConfigDialog({
  target,
  onClose,
}: {
  target: ConfigTarget | null;
  onClose: () => void;
}) {
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [providers, setProviders] = useState<ProviderEntry[]>([]);
  const [selectedId, setSelectedId] = useState<string>("");
  const [usage, setUsage] = useState<Record<string, number>>({});
  const [usageMonth, setUsageMonth] = useState<string>("");
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testQuery, setTestQuery] = useState("");
  const [testLimit, setTestLimit] = useState(5);
  const [testResult, setTestResult] = useState<
    { ok: boolean; title: string; body: string } | null
  >(null);
  const [err, setErr] = useState<string | null>(null);

  // 从 schema 里取 providers 描述(若无则非 providers 型,退回普通表单)
  const providerSchema = useMemo(
    () => target?.schema.find((s) => s.type === "providers") ?? null,
    [target],
  );
  const isProviders = !!providerSchema;

  // 初始化:合并已存 providers 值 + manifest 内置模板
  useEffect(() => {
    if (!target) return;
    setValues(target.values || {});
    setErr(null);
    setTestResult(null);

    if (!providerSchema) {
      setProviders([]);
      setSelectedId("");
      return;
    }
    const templates: ProviderManifestItem[] = providerSchema.items ?? [];
    const stored: unknown[] = Array.isArray(target.values?.providers)
      ? (target.values.providers as unknown[])
      : [];
    // 已存条目按 id 索引(自定义优先;内置若被改动则采用已存版)
    const byId = new Map<string, ProviderEntry>();
    for (const t of templates) {
      byId.set(t.id, { ...t, enabled: true, present: false });
    }
    for (const s of stored) {
      if (s && typeof s === "object") {
        const obj = s as Record<string, unknown>;
        const id = String(obj.id ?? "");
        if (id) {
          const base = byId.get(id) ?? ({} as ProviderEntry);
          byId.set(id, { ...base, ...obj, id, present: true } as ProviderEntry);
        }
      }
    }
    // 已存里出现、但模板没有的自定义供应商(全新),补进去
    for (const s of stored) {
      if (s && typeof s === "object") {
        const obj = s as Record<string, unknown>;
        const id = String(obj.id ?? "");
        if (id && !byId.has(id)) {
          byId.set(id, { ...(obj as ProviderEntry), id, builtin: false });
        }
      }
    }
    const list = Array.from(byId.values());
    // 已删墓碑排到最后,列表主序保持可用家在前
    list.sort((a, b) => Number(!!a.deleted) - Number(!!b.deleted));
    setProviders(list);
    finalizeSelection(list, target.values, usage);
  }, [target, providerSchema]);

  // 默认选中第一个可用(未删)家;停用家可选,已删家不默认选中
  function finalizeSelection(
    list: ProviderEntry[],
    vals: Record<string, unknown>,
    u: Record<string, number>,
  ) {
    if (list.length) {
      const live = list.filter((p) => !p.deleted);
      const storedIds = new Set(
        (Array.isArray(vals.providers) ? vals.providers : []).map(
          (x: unknown) => (x as Record<string, unknown>).id as string,
        ),
      );
      const firstStored = live.find((p) => storedIds.has(p.id)) ?? live[0];
      if (firstStored) setSelectedId(firstStored.id);
    }
  }

  // 拉取用量(进度条)
  useEffect(() => {
    if (!target || !isProviders) return;
    let alive = true;
    api.mcp
      .getUsage(target.name)
      .then((r) => {
        if (!alive) return;
        setUsage(r.usage?.providers ?? {});
        setUsageMonth(r.usage?.month ?? "");
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [target, isProviders]);

  if (!target) return null;

  const selected = providers.find((p) => p.id === selectedId) ?? null;

  const setField = (id: string, field: keyof ProviderManifestItem, val: string | boolean | number) =>
    setProviders((prev) =>
      prev.map((p) => (p.id === id ? { ...p, [field]: val } : p)),
    );

  // 新增家:生成随机 id,强制走通用引擎(std)
  const handleAdd = () => {
    const id = `custom_${Date.now().toString(36)}`;
    const fresh: ProviderEntry = {
      id,
      name: "新供应商",
      builtin: false,
      enabled: true,
      endpoint: "",
      method: "GET",
      auth: "header",
      auth_name: "X-API-KEY",
      key: "",
      query_param: "q",
      limit_param: "",
      results_path: "/results",
      title_field: "title",
      url_field: "url",
      desc_field: "description",
      parse: "std",
      quota: 0,
      present: true,
    };
    setProviders((prev) => [...prev, fresh]);
    setSelectedId(id);
  };

  const handleTest = async () => {
    if (!selected || !testQuery) return;
    setTesting(true);
    setTestResult(null);
    setErr(null);
    try {
      const r = await api.mcp.testSearch(
        target.name,
        selected.id,
        testQuery,
        testLimit || 5,
      );
      if (!r.ok) {
        setTestResult({ ok: false, title: "测试失败", body: r.error ?? "未知错误" });
        return;
      }
      const res = r.result;
      if (!res?.success) {
        setTestResult({
          ok: false,
          title: `${selected.id} 测试失败`,
          body: res?.error ?? "无返回",
        });
      } else {
        const lines = (res.results ?? [])
          .slice(0, 10)
          .map((it, i) => `${i + 1}. ${it.title ?? ""}\n   ${it.url ?? ""}\n   ${it.description ?? ""}`)
          .join("\n");
        setTestResult({
          ok: true,
          title: `${selected.name} · ${res.count ?? 0} 条 · ${res.timing_ms ?? 0}ms`,
          body: lines || "(无结果)",
        });
      }
      // 测试成功会记一次用量,顺手刷新
      if (res?.success) {
        api.mcp.getUsage(target.name).then((qr) => {
          if (qr.ok && qr.usage?.providers) setUsage(qr.usage.providers);
        });
      }
    } catch (e) {
      setTestResult({
        ok: false,
        title: "测试请求失败",
        body: String(e instanceof Error ? e.message : e),
      });
    } finally {
      setTesting(false);
    }
  };

  const handleSave = async () => {
    setSaving(true);
    setErr(null);
    try {
      if (isProviders) {
        // 保存 providers 列表(过滤掉空 id;present 是 UI 专用字段不落盘,
        // enabled/deleted 墓碑保留——插件侧据此跳过停用家/抑制内置回填)
        const persist = providers
          .filter((p) => p.id)
          .map(({ present: _present, ...rest }) => rest);
        setValues((prev) => ({ ...prev, providers: persist }));
        await api.mcp.saveConfig(target.name, {
          ...values,
          providers: persist,
        });
      } else {
        await api.mcp.saveConfig(target.name, values);
      }
      onClose();
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setSaving(false);
    }
  };

  // 若 providers 型 → 左右分栏新布局
  if (isProviders) {
    return (
      <Dialog open onOpenChange={(v) => !v && onClose()}>
        <DialogContent className="flex h-[min(82vh,620px)] flex-col overflow-hidden sm:max-w-[798px]">
          <DialogHeader className="shrink-0">
            <div className="flex items-start justify-between gap-3 pr-4">
              <div className="min-w-0">
                <DialogTitle>配置 · {target.name}</DialogTitle>
                <DialogDescription>
                  左侧点选供应商编辑,「新增」接入全新搜索服务(通用引擎)。
                </DialogDescription>
              </div>
              <Button
                variant="outline"
                size="sm"
                className="mt-0.5 shrink-0 gap-1"
                onClick={handleAdd}
                title="新增供应商(通用引擎)"
                data-slot="provider-add"
              >
                <PlusIcon className="size-3.5" /> 新增
              </Button>
            </div>
          </DialogHeader>

          {/* 统一容器:固定高度 flex 列,左右在内部滚动,对话框高度恒定 */}
          <div className="min-h-0 flex-1 overflow-hidden rounded-xl border">
            <div className="grid h-full grid-cols-[minmax(0,210px)_minmax(0,1fr)]">
              {/* ===== 左:用量进度条(内部滚动,滚动条隐藏) ===== */}
              <div className="bo-scroll-hidden flex h-full min-h-0 flex-col overflow-y-auto border-r p-2.5">
                <div className="flex shrink-0 items-center justify-between">
                  <Label className="text-xs font-semibold">用量(本月)</Label>
                  {usageMonth ? (
                    <span className="text-muted-foreground text-[10.5px] font-mono">
                      {usageMonth}
                    </span>
                  ) : null}
                </div>
                {/* 列表在左栏内部滚动 */}
                <div className="mt-1.5 space-y-1.5">
                {providers.map((p) => {
                  const used = usage[p.id] ?? 0;
                  const quota = p.quota ?? 0;
                  const ratio = quota > 0 ? Math.min(1, used / quota) : 0;
                  const pct = Math.round(ratio * 100);
                  const color =
                    ratio >= 1
                      ? "bg-rose-500"
                      : ratio >= 0.8
                        ? "bg-amber-500"
                        : "bg-emerald-500";
                  const off = p.enabled === false || !!p.deleted;
                  return (
                    <button
                      key={p.id}
                      type="button"
                      onClick={() => setSelectedId(p.id)}
                      className={`w-full text-left rounded-md border px-2.5 py-1.5 transition-colors ${
                        selected?.id === p.id
                          ? "border-primary/60 bg-primary/5"
                          : "border-border bg-card/40 hover:bg-muted/40"
                      } ${off ? "opacity-60" : ""}`}
                    >
                      <div className="flex items-baseline justify-between gap-2">
                        <span className={`truncate text-[12px] font-medium ${off ? "text-muted-foreground line-through decoration-border" : ""}`}>
                          {p.name || p.id}
                        </span>
                        <span className="text-muted-foreground shrink-0 text-[10.5px] font-mono">
                          {quota > 0 ? `${used}/${quota}` : `${used} 次`}
                          {quota > 0 ? ` · ${pct}%` : ""}
                        </span>
                      </div>
                      <div className="mt-1 flex items-center gap-1.5">
                        {p.deleted ? (
                          <span className="shrink-0 rounded border border-border px-1 font-mono text-[9.5px] text-muted-foreground">
                            已删除
                          </span>
                        ) : p.enabled === false ? (
                          <span className="shrink-0 rounded border border-[var(--state-warn-border)] bg-[var(--state-warn-bg)] px-1 font-mono text-[9.5px] text-[var(--state-warn-fg)]">
                            已停用
                          </span>
                        ) : null}
                        <div className="h-1 min-w-0 flex-1 overflow-hidden rounded-full bg-muted">
                          <div
                            className={`h-full rounded-full transition-all ${color}`}
                            style={{ width: `${quota > 0 ? pct : 0}%` }}
                          />
                        </div>
                      </div>
                    </button>
                  );
                })}
              </div>
            </div>

            {/* ===== 右:供应商配置(内部滚动,滚动条隐藏) ===== */}
            <div className="bo-scroll-hidden flex min-h-0 min-w-0 flex-col gap-2.5 overflow-y-auto p-2.5">
              {/* 通用设置(全局,与供应商无关) */}
              <div className="rounded-lg border p-2.5">
                <div className="flex items-center justify-between">
                  <Label className="text-[11px] font-semibold">通用设置</Label>
                  <span className="text-muted-foreground text-[10.5px]">指最终返回,非每家条数</span>
                </div>
                <div className="mt-1.5 flex flex-wrap items-center gap-x-4 gap-y-1">
                  {target.schema
                    .filter((s) => s.type !== "providers")
                    .map((item) => (
                      <div key={item.key} className="space-y-0.5">
                        <Label className="text-[11px] font-medium">
                          {item.key === "default_limit"
                            ? "最终返回条数"
                            : item.label || item.key}
                        </Label>
                        <div className="h-7">
                          <Input
                            type={item.type === "range" ? "number" : "text"}
                            value={String(values[item.key] ?? item.default ?? "")}
                            onChange={(e) =>
                              setValues((prev) => ({ ...prev, [item.key]: e.target.value }))
                            }
                            className="h-7 w-24 font-mono text-[12px]"
                          />
                        </div>
                      </div>
                    ))}
                </div>
              </div>
              {/* 选中家:已删 → 恢复卡;未删 → 生效开关 + 删除 + 编辑表单 */}
              {selected && selected.deleted ? (
                <div className="rounded-lg border border-dashed p-4">
                  <div className="flex items-center justify-between gap-3">
                    <div className="min-w-0">
                      <div className="text-[12.5px] font-medium">
                        {selected.builtin ? "内置供应商" : "供应商"}「
                        {selected.name || selected.id}」已删除
                      </div>
                      <div className="text-muted-foreground mt-0.5 text-[11px]">
                        删除后不参与搜索轮转;历史用量保留,恢复后按原配置继续生效。
                      </div>
                    </div>
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 shrink-0 text-[12px]"
                      onClick={() =>
                        setProviders((prev) =>
                          prev.map((p) =>
                            p.id === selected.id
                              ? { ...p, deleted: false, enabled: true }
                              : p,
                          ),
                        )
                      }
                      data-slot="provider-restore"
                    >
                      恢复
                    </Button>
                  </div>
                </div>
              ) : selected ? (
                <div className="rounded-lg border p-3">
                  <div className="mb-2.5 flex items-center justify-between gap-2">
                    <Label className="text-xs font-semibold">
                      {selected.builtin ? "编辑内置" : "编辑自定义"}:{" "}
                      {selected.name || selected.id}
                    </Label>
                    <div className="flex shrink-0 items-center gap-2.5">
                      <span className="text-muted-foreground flex items-center gap-1.5 text-[11px]">
                        <Switch
                          checked={selected.enabled !== false}
                          onCheckedChange={(v) =>
                            setField(selected.id, "enabled", v)
                          }
                          data-slot="provider-enabled"
                        />
                        {selected.enabled === false ? "已停用" : "生效中"}
                      </span>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="text-destructive h-6 px-2 text-[11px]"
                        title={
                          selected.builtin
                            ? "删除该内置供应商(存墓碑,可恢复)"
                            : "移除该自定义供应商"
                        }
                        data-slot="provider-remove"
                        onClick={() => {
                          if (selected.builtin) {
                            // 内置:存墓碑条目抑制插件「缺失内置回填」,可恢复
                            setProviders((prev) =>
                              prev.map((p) =>
                                p.id === selected.id
                                  ? { ...p, deleted: true }
                                  : p,
                              ),
                            );
                          } else {
                            setProviders((prev) =>
                              prev.filter((p) => p.id !== selected.id),
                            );
                          }
                          const next = providers.find(
                            (p) => p.id !== selected.id && !p.deleted,
                          );
                          setSelectedId(next?.id ?? "");
                        }}
                      >
                        {selected.builtin ? "删除" : "移除"}
                      </Button>
                    </div>
                  </div>
                  <div className="grid grid-cols-2 gap-3 text-[12px]">
                    <FormField
                      label="显示名"
                      value={selected.name}
                      onChange={(v) => setField(selected.id, "name", v)}
                    />
                    <FormField
                      label="接口地址"
                      value={selected.endpoint}
                      mono
                      placeholder="https://api.example.com/search"
                      onChange={(v) => setField(selected.id, "endpoint", v)}
                    />
                    <div className="space-y-1">
                      <Label className="text-[11px]">请求方式</Label>
                      <Select
                        value={selected.method}
                        onValueChange={(v) => v && setField(selected.id, "method", v)}
                      >
                        <SelectTrigger size="sm" className="h-7 w-full text-[12px]">
                          <SelectValue placeholder="方式" />
                        </SelectTrigger>
                        <SelectContent className="rounded-lg">
                          <SelectItem value="GET">GET</SelectItem>
                          <SelectItem value="POST">POST</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <FormField
                      label="API Key(逗号多把)"
                      value={selected.key ?? ""}
                      password
                      mono
                      onChange={(v) => setField(selected.id, "key", v)}
                    />
                    <div className="space-y-1">
                      <Label className="text-[11px]">Key 传法</Label>
                      <Select
                        value={selected.auth}
                        onValueChange={(v) => v && setField(selected.id, "auth", v)}
                      >
                        <SelectTrigger size="sm" className="h-7 w-full text-[12px]">
                          <SelectValue placeholder="传法" />
                        </SelectTrigger>
                        <SelectContent className="rounded-lg">
                          <SelectItem value="header">请求头</SelectItem>
                          <SelectItem value="bearer">Bearer Token</SelectItem>
                          <SelectItem value="query">参数 (query/body)</SelectItem>
                          <SelectItem value="none">无</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    {selected.auth === "header" || selected.auth === "query" ? (
                      <FormField
                        label={selected.auth === "header" ? "请求头名" : "Key 参数名"}
                        value={selected.auth_name}
                        mono
                        placeholder={selected.auth === "header" ? "X-API-KEY" : "api_key"}
                        onChange={(v) => setField(selected.id, "auth_name", v)}
                      />
                    ) : null}
                    <FormField
                      label="query 参数名"
                      value={selected.query_param}
                      mono
                      placeholder="q"
                      onChange={(v) => setField(selected.id, "query_param", v)}
                    />
                    <FormField
                      label="limit 参数名(可空)"
                      value={selected.limit_param}
                      mono
                      placeholder="num / max_results"
                      onChange={(v) => setField(selected.id, "limit_param", v)}
                    />
                    <FormField
                      label="结果数组位置(JSON 路径)"
                      value={selected.results_path}
                      mono
                      placeholder="/organic"
                      onChange={(v) => setField(selected.id, "results_path", v)}
                    />
                    <div className="space-y-1">
                      <Label className="text-[11px]">月度配额(0=不限)</Label>
                      <Input
                        type="number"
                        min={0}
                        value={String(selected.quota ?? 0)}
                        onChange={(e) =>
                          setField(selected.id, "quota", Number(e.target.value) || 0)
                        }
                        className="h-7 font-mono text-[12px]"
                      />
                    </div>
                    <FormField
                      label="标题字段"
                      value={selected.title_field}
                      mono
                      placeholder="title"
                      onChange={(v) => setField(selected.id, "title_field", v)}
                    />
                    <FormField
                      label="链接字段"
                      value={selected.url_field}
                      mono
                      placeholder="url"
                      onChange={(v) => setField(selected.id, "url_field", v)}
                    />
                    <FormField
                      label="摘要字段"
                      value={selected.desc_field}
                      mono
                      placeholder="snippet"
                      onChange={(v) => setField(selected.id, "desc_field", v)}
                    />
                  </div>

                  {/* 测试按钮 */}
                  <div className="mt-3 flex items-center gap-2 border-t pt-3">
                    <Input
                      value={testQuery}
                      onChange={(e) => setTestQuery(e.target.value)}
                      placeholder="输入测试关键词…"
                      className="h-8 flex-1 text-[12px]"
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void handleTest();
                      }}
                    />
                    <Input
                      type="number"
                      min={1}
                      max={20}
                      value={String(testLimit || 5)}
                      onChange={(e) => setTestLimit(Number(e.target.value) || 5)}
                      className="h-8 w-16 text-[12px]"
                      title="返回条数"
                    />
                    <Button
                      variant="secondary"
                      size="sm"
                      className="h-8 shrink-0 gap-1.5"
                      onClick={() => void handleTest()}
                      disabled={testing || !testQuery}
                    >
                      {testing ? (
                        <Loader2Icon className="animate-spin size-3.5" />
                      ) : (
                        <PlayIcon className="size-3.5" />
                      )}
                      测试真搜
                    </Button>
                  </div>

                  {testResult ? (
                    <div
                      className={cn(
                        "mt-2.5 rounded-md border p-2.5 text-[12px]",
                        testResult.ok
                          ? "border-[var(--state-success-border)] bg-[var(--state-success-bg)]"
                          : "border-[var(--state-error-border)] bg-[var(--state-error-bg)]",
                      )}
                    >
                      <div className="mb-1 flex items-center justify-between font-medium">
                        <span
                          className={
                            testResult.ok
                              ? "text-[var(--state-success-fg)]"
                              : "text-[var(--state-error-fg)]"
                          }
                        >
                          {testResult.ok ? "✓ " : "✕ "}
                          {testResult.title}
                        </span>
                        <button
                          className="text-muted-foreground transition-colors hover:text-foreground"
                          onClick={() => setTestResult(null)}
                          title="收起结果"
                        >
                          <XIcon className="size-3.5" />
                        </button>
                      </div>
                      <pre className="max-h-48 overflow-auto whitespace-pre-wrap font-mono text-[11px] leading-relaxed">
                        {testResult.body}
                      </pre>
                    </div>
                  ) : null}
                </div>
              ) : (
                <div className="text-muted-foreground rounded-lg border border-dashed p-6 text-center text-[12px]">
                  点「新增」接入全新搜索供应商
                </div>
              )}
            </div>
            </div>
          </div>

          {err ? <div className="text-destructive text-xs">{err}</div> : null}
          <DialogFooter className="shrink-0">
            <Button variant="outline" onClick={onClose} disabled={saving}>
              取消
            </Button>
            <Button onClick={() => void handleSave()} disabled={saving}>
              {saving ? <Loader2Icon className="animate-spin" /> : null} 保存配置
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }

  // ===== 普通(非 providers)插件:保持原瀑布式表单 =====
  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>配置 · {target.name}</DialogTitle>
          <DialogDescription>该插件声明了可调节的运行配置。</DialogDescription>
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
            {saving ? <Loader2Icon className="animate-spin" /> : null} 保存配置
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// 一个小型表单字段(标签 + 单行输入),供 providers 编辑复用
function FormField({
  label,
  value,
  onChange,
  placeholder,
  mono,
  password,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  mono?: boolean;
  password?: boolean;
}) {
  return (
    <div className="space-y-1">
      <Label className="text-[11px]">{label}</Label>
      <Input
        type={password ? "password" : "text"}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        className={`h-7 text-[12px] ${mono ? "font-mono" : ""}`}
      />
    </div>
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
              className="bo-select border-input bg-background h-8 rounded-lg border border-border bg-muted/40 px-2 font-mono text-[12px] text-foreground outline-none focus:ring-2 focus:ring-ring"
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
