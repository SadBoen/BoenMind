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


} from "lucide-react";
import {
  api,
  type Capability,
  type McpListResult,
  type McpServer,
} from "../api";
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
import { cn } from "@/lib/utils";
import { STORAGE_KEYS, storage } from "@/lib/storage";
import { Tooltip } from "radix-ui";
import { ServerConfigDialog } from "./ServerConfigDialog";
import { McpDialog } from "./McpDialog";
import { type ConfigTarget, type McpCandidatesResult, type Draft, emptyDraft, toDraft, fromDraft } from "./types";
import { type ColKey, TABLE_COLUMNS, loadColWidths } from "./columns";

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
  /** ADR-0023:官方随包来源但最新官方清单已不含 → 建议删除 */
  deprecated?: boolean;
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
  // ADR-0023:物理删除确认弹窗目标(包含来源与是否废弃，便于精准提示)
  const [purgeTarget, setPurgeTarget] = useState<{
    name: string;
    command?: string;
    origin?: "bundled" | "data" | "unknown";
    deprecated?: boolean;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // 表格列宽状态(支持记忆与拖动)
  const [colWidths, setColWidths] = useState<Record<ColKey, number>>(loadColWidths);
  const colWidthsRef = useRef(colWidths);
  // 镜像同步放 effect(React Compiler 规则:渲染期不可写 ref;拖拽读取最新列宽)
  useEffect(() => {
    colWidthsRef.current = colWidths;
  }, [colWidths]);

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

    // 2. 外部 MCP 插件 (通过 mcp.json 与真实后端探活驱动,绝无静态硬编码)
    if (mcpData?.servers) {
      for (const s of mcpData.servers) {
        const st = statusMap[s.name];
        const isOk = st?.ok ?? false;
        // 2026-09-05 回看修复:移除 tool_N 伪造清单——探活只报数量而未返回
        // 清单时,不得编造工具名,如实显示数量、清单留空
        const tools: ToolInfo[] =
          st?.tool_list && st.tool_list.length > 0 ? st.tool_list : [];
        const entry = mcpData.entries?.find((e) => e.server.name === s.name);

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
          deprecated: entry?.deprecated ?? false,
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
    if (
      !confirm(
        `确定卸载 MCP 插件「${name}」？配置将移除并即时下线(本地文件保留)。系统不会在重启或升级时自动静默启用该插件，您随时可在「扫描插件」中重新接入。`,
      )
    )
      return;
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

  // ADR-0023:卸载并物理删除插件文件(警告弹窗确认后)
  const handlePurge = async (name: string) => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.mcp.purge(name);
      setPurgeTarget(null);
      if (r.ok) {
        const aside = r.renamed_aside.length
          ? `;${r.renamed_aside.length} 个运行中的文件已让位改名`
          : "";
        setNotice(`已删除「${name}」:${r.deleted.length} 个文件${aside}`);
      } else {
        setError(`删除「${name}」部分失败:${r.errors.join(";")}`);
      }
      await loadData();
      await refreshStatus();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
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
                      {item.deprecated ? (
                        <span
                          className="shrink-0 rounded border border-[var(--state-warn-border)] bg-[var(--state-warn-bg)] px-1 font-mono text-[9.5px] text-[var(--state-warn-fg)]"
                          title="最新官方版本已不包含此插件,建议用「删除」清理"
                        >
                          已不随包
                        </span>
                      ) : null}
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
                          className="h-7 px-2 text-[11.5px]"
                          onClick={() => void handleRemove(item.name)}
                        >
                          卸载
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={busy}
                          className="h-7 px-2 text-[11.5px] text-destructive hover:bg-destructive/10"
                          title="卸载并物理删除插件文件(含警告确认)"
                          data-slot="mcp-purge"
                          onClick={() =>
                            setPurgeTarget({
                              name: item.name,
                              command: item.serverRef?.command,
                              origin: mcpEntry?.origin,
                              deprecated: mcpEntry?.deprecated,
                            })
                          }
                        >
                          删除
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
                        {c.tombstoned && !c.registered ? (
                          <span className="text-muted-foreground text-xs ml-1">
                            {c.source === "bundled"
                              ? "(未自动启用 · 可重新接入)"
                              : "(已移除 · 可恢复接入)"}
                          </span>
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
                        setError(null);
                        try {
                          // ADR-0023:批准即自动上线(后端热重载),前端只刷新
                          const r = await api.mcp.approve(c.name);
                          setScanResult(null);
                          const tools = r.reload?.tools;
                          setNotice(
                            typeof tools === "number"
                              ? `「${c.name}」已批准并自动上线 · ${tools} 个工具`
                              : (r.note ?? `「${c.name}」已批准`),
                          );
                          await loadData();
                          await refreshStatus();
                        } catch (e) {
                          setError(String(e instanceof Error ? e.message : e));
                        } finally {
                          setApproving(null);
                        }
                      }}
                    >
                      {c.registered
                        ? "已批准"
                        : c.tombstoned
                          ? c.source === "bundled"
                            ? "批准接入"
                            : "批准恢复"
                          : "批准接入"}
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

      {/* ADR-0023:物理删除警告栏(卸载 + 删原文件,不可撤销) */}
      {purgeTarget ? (
        <Dialog open onOpenChange={(v) => !v && setPurgeTarget(null)}>
          <DialogContent className="sm:max-w-md" data-slot="mcp-purge-dialog">
            <DialogHeader>
              <DialogTitle className="text-destructive">
                删除插件「{purgeTarget.name}」
              </DialogTitle>
              <DialogDescription>
                此操作会物理删除插件文件,不可撤销:
              </DialogDescription>
            </DialogHeader>
            <ul className="text-muted-foreground list-disc space-y-1 pl-4 text-[12px]">
              <li className="font-mono break-all">
                {purgeTarget.command || "插件可执行文件(未登记路径则仅清理声明)"}
              </li>
              <li>
                声明清单 manifests/{purgeTarget.name}.manifest.json 与每插件配置
                config/mcp-{purgeTarget.name}.json(如存在)
              </li>
              {purgeTarget.deprecated ? (
                <li>
                  彻底移除：官方最新版本已不再随包此插件，本地文件将全部清除；若未来官方重新引入同名插件，系统不会静默自启，您可在「扫描插件」中按需重新接入
                </li>
              ) : purgeTarget.origin === "bundled" ? (
                <li>
                  防静默自启：记录不自动启用状态；后续系统版本更新即使覆盖带来新文件，也不会擅自静默启动，您随时可在「扫描插件」中主动重新批准接入
                </li>
              ) : (
                <li>
                  完全抹除：彻底清理此插件的可执行文件及本地配置文件
                </li>
              )}
            </ul>
            <DialogFooter>
              <Button variant="outline" onClick={() => setPurgeTarget(null)}>
                取消
              </Button>
              <Button
                variant="destructive"
                disabled={busy}
                data-slot="mcp-purge-confirm"
                onClick={() => void handlePurge(purgeTarget.name)}
              >
                {busy ? "删除中…" : "确认删除"}
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
