// 统一插件中心：整合「系统内置能力」与「外部 MCP 插件」
// 采用表格式呈现，顶部提供【全部 / 内置 / 外部】快速筛选，保留完整的扫描、配置与操作能力。
// #22 拆分:本文件为数据与装配层;表格行(PluginTableRow)、扫描候选对话框
// (ScanCandidatesDialog)、stderr 弹窗(StderrDialog)、ServerConfigDialog/
// McpDialog 各自成文件;行为回调在此统一驱动 API。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Loader2Icon,
  PlusIcon,
  RefreshCwIcon,
  ScanSearchIcon,
} from "lucide-react";
import {
  api,
  type Capability,
  type McpListResult,
  type WasmPlugin,
} from "../api";
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
import { ServerConfigDialog } from "./ServerConfigDialog";
import { McpDialog } from "./McpDialog";
import { PluginTableRow } from "./PluginTableRow";
import { ScanCandidatesDialog } from "./ScanCandidatesDialog";
import { StderrDialog, type StderrViewState } from "./StderrDialog";
import {
  type ConfigTarget,
  type McpCandidatesResult,
  type Draft,
  type ToolInfo,
  type TablePluginItem,
  BUILTIN_DESC,
  emptyDraft,
  toDraft,
  fromDraft,
} from "./types";
import { type ColKey, TABLE_COLUMNS, loadColWidths } from "./columns";

export function PluginsPage() {
  const [filter, setFilter] = useState("");
  const [typeFilter, setTypeFilter] = useState<"all" | "builtin" | "external" | "wasm">("all");
  const [builtinList, setBuiltinList] = useState<Capability[]>([]);
  const [mcpData, setMcpData] = useState<McpListResult | null>(null);
  const [wasmList, setWasmList] = useState<WasmPlugin[]>([]);
  const [statusMap, setStatusMap] = useState<
    Record<string, { ok: boolean; tools?: number; tool_list?: ToolInfo[]; error?: string }>
  >({});

  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [scanDisclosed, setScanDisclosed] = useState(false);
  const [reloading, setReloading] = useState(false);
  const [scanResult, setScanResult] = useState<McpCandidatesResult | null>(null);
  const [approving, setApproving] = useState<string | null>(null);
  const [configTarget, setConfigTarget] = useState<ConfigTarget | null>(null);
 // issue #28:子进程 stderr 回看弹窗
  const [stderrView, setStderrView] = useState<StderrViewState | null>(null);
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
      const [bRes, mRes, wRes] = await Promise.all([
        api.capabilities().catch(() => ({ builtin: [] })),
        api.mcp.list().catch(() => null),
        api.plugins.list().catch(() => null),
      ]);
      setBuiltinList(bRes.builtin ?? []);
      setMcpData(mRes);
      setWasmList(wRes?.plugins ?? []);
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

 // 统一列表聚合
  const tableItems: TablePluginItem[] = useMemo(() => {
    const list: TablePluginItem[] = [];
 // ADR-0042:wasm 插件能力也会出现在 /admin/capabilities 的 builtin 启动快照里
 // (组合根把插件能力并入 capabilities),故内置面须跳过它们,改由下方 wasm 面
 // 以正确类别 + 卸载按钮呈现——真实浏览器手测发现的重复/错标。
    const wasmNames = new Set(wasmList.map((w) => w.capability));

 // 1. 系统内置能力(排除 wasm 插件能力,避免重复与错标「禁卸载」)
    for (const b of builtinList) {
      if (wasmNames.has(b.name)) continue;
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
        pluginKind: b.plugin_kind,
        pluginId: b.plugin_id,
        pluginVersion: b.plugin_version,
      });
    }

 // 2. 外部 MCP 插件 (通过 mcp.json 与真实后端探活驱动,绝无静态硬编码)
    if (mcpData?.servers) {
      for (const s of mcpData.servers) {
        const st = statusMap[s.name];
        const isOk = st?.ok ?? false;
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

 // 3. 通用 wasm 插件(ADR-0042:config/plugins.json;增删改即热重载)
    for (const w of wasmList) {
      list.push({
        id: `wasm:${w.capability}`,
        name: w.capability,
        type: "wasm",
        detail:
          w.description ??
          `wasm 插件 · ${w.effect ?? "read-only"}${w.version ? ` · v${w.version}` : ""}`,
        tools: [{ name: w.capability, description: w.description }],
        isOnline: true,
        wasmRef: w,
 // wasm 插件的 provider 恒声明 Tool 身份(ADR-0041/0045)
        pluginKind: "tool",
        pluginId: w.provider ?? w.capability,
        pluginVersion: w.version,
      });
    }

    return list;
  }, [builtinList, mcpData, statusMap, wasmList]);

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

 // 当前筛选面为空(如卸载掉最后一个 wasm 插件而筛选仍停在 wasm)→ 回落显示「全部」,
 // 避免空表无从解释。用派生值而非 useEffect+setState(后者会触发 lint 级联渲染告警)。
  const effectiveTypeFilter =
    typeFilter !== "all" && filteredItems.length === 0 && tableItems.length > 0
      ? "all"
      : typeFilter;
  const shownItems =
    effectiveTypeFilter === typeFilter
      ? filteredItems
      : tableItems.filter((item) => {
          if (filter.trim()) {
            const kw = filter.trim().toLowerCase();
            return (
              item.name.toLowerCase().includes(kw) || item.detail.toLowerCase().includes(kw)
            );
          }
          return true;
        });

  const scanPlugins = async () => {
 // ADR-0035:扫描会以 --self-describe 运行候选目录内的可执行文件。
 // 首次扫描前显式披露该执行面(利用候选必须被运行才能自报 name)。
    if (
      !scanDisclosed &&
      !confirm(
        "扫描需要以 --self-describe 参数运行插件目录内的可执行文件,以读取其自报声明(识别插件身份的唯一方式)。\n\n仅「批准接入」后才写入配置并上线。是否继续扫描?",
      )
    ) {
      return;
    }
    setScanDisclosed(true);
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
 // ADR-0042:wasm 插件卸载 = 移除声明并即时热重载(按 capability 摘除)
    const wasmItem = tableItems.find((it) => it.type === "wasm" && it.name === name);
    if (wasmItem) {
      if (!confirm(`确定卸载 wasm 插件「${name}」?声明将移除并即时下线(免重启)。`)) return;
      setBusy(true);
      setError(null);
      try {
        const r = await api.plugins.remove(name);
        setNotice(r.note ?? `「${name}」已卸载`);
        await loadData();
      } catch (e) {
        setError(String(e instanceof Error ? e.message : e));
      } finally {
        setBusy(false);
      }
      return;
    }
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

 // 扫描候选批准:ADR-0023 批准即自动上线(后端热重载),前端只刷新
  const handleApproveCandidate = async (c: McpCandidatesResult["candidates"][number]) => {
    setApproving(c.name);
    setError(null);
    try {
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
  };

 // 行为回调:表格行动作(数据面仍以本页 mcpData 为唯一来源)
  const findEntry = (name: string) => mcpData?.entries?.find((e) => e.server.name === name);

  const handleRowEdit = (item: TablePluginItem) => {
    setDraft(toDraft(item.serverRef!));
  };

  const handleRowConfig = (item: TablePluginItem) => {
    const mcpEntry = findEntry(item.name);
    api.mcp.getConfig(item.name).then((cfg) => {
      setConfigTarget({
        name: item.name,
        schema: mcpEntry?.manifest?.config_schema ?? [],
        values: cfg.values,
      });
    });
  };

  const handleRowStderr = (name: string) => {
    setStderrView({ name, loading: true, lines: [] });
    api.mcp.getStderr(name, 200).then((r) => {
      setStderrView({
        name,
        loading: false,
        lines: r.lines ?? [],
        error: r.ok ? undefined : r.error,
      });
    });
  };

  const handleStderrRefresh = () => {
    if (!stderrView) return;
    setStderrView({ ...stderrView, loading: true });
    api.mcp.getStderr(stderrView.name, 200).then((r) => {
      setStderrView({
        name: stderrView.name,
        loading: false,
        lines: r.lines ?? [],
        error: r.ok ? undefined : r.error,
      });
    });
  };

  const handleRowPurge = (item: TablePluginItem) => {
    const mcpEntry = findEntry(item.name);
    setPurgeTarget({
      name: item.name,
      command: item.serverRef?.command,
      origin: mcpEntry?.origin,
      deprecated: mcpEntry?.deprecated,
    });
  };

  return (
    <div className="flex flex-col gap-4">
      {/* 顶部标题与操作栏 */}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-[16px] font-semibold text-foreground">插件与能力中心</h2>
          <p className="text-muted-foreground mt-0.5 text-[12.5px]">
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
            variant={effectiveTypeFilter === "all" ? "default" : "ghost"}
            className="h-6.5 px-2.5 text-[11.5px]"
            onClick={() => setTypeFilter("all")}
          >
            全部 ({tableItems.length})
          </Button>
          <Button
            size="sm"
            variant={effectiveTypeFilter === "builtin" ? "default" : "ghost"}
            className="h-6.5 px-2.5 text-[11.5px]"
            onClick={() => setTypeFilter("builtin")}
          >
            内置 ({tableItems.filter((it) => it.type === "builtin").length})
          </Button>
          <Button
            size="sm"
            variant={effectiveTypeFilter === "external" ? "default" : "ghost"}
            className="h-6.5 px-2.5 text-[11.5px]"
            onClick={() => setTypeFilter("external")}
          >
            外部 ({mcpData?.servers?.length ?? 0})
          </Button>
          <Button
            size="sm"
            variant={effectiveTypeFilter === "wasm" ? "default" : "ghost"}
            className="h-6.5 px-2.5 text-[11.5px]"
            onClick={() => setTypeFilter("wasm")}
          >
            Wasm ({wasmList.length})
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
            {shownItems.map((item) => (
              <PluginTableRow
                key={item.id}
                item={item}
                mcpEntry={findEntry(item.name)}
                busy={busy}
                onEdit={handleRowEdit}
                onConfig={handleRowConfig}
                onStderr={handleRowStderr}
                onRemove={(name) => void handleRemove(name)}
                onPurge={handleRowPurge}
              />
            ))}

            {shownItems.length === 0 ? (
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

      {/* issue #28:子进程 stderr 回看 */}
      {stderrView ? (
        <StderrDialog
          view={stderrView}
          onClose={() => setStderrView(null)}
          onRefresh={handleStderrRefresh}
        />
      ) : null}

      {/* 插件扫描发现对话框 */}
      {scanResult ? (
        <ScanCandidatesDialog
          result={scanResult}
          approving={approving}
          onClose={() => setScanResult(null)}
          onApprove={(c) => void handleApproveCandidate(c)}
        />
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
                {purgeTarget.command || "插件可执行文件(未登记路径则按扫描路径定位)"}
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
