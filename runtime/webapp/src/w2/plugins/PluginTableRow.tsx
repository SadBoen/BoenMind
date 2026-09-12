//! 插件中心表格行渲染(#22 拆分:自 PluginsPage.tsx 机械移入)。
//! 单行 = 名称/描述 + 类别徽标 + 工具标签(气泡防撑破) + 操作按钮组;
//! 行为经回调上抛,数据获取仍由装配层(PluginsPage)统一驱动。

import { ShieldCheck, Globe, Wrench, Boxes } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tooltip } from "radix-ui";
import { cn } from "@/lib/utils";
import type { McpListResult } from "../api";
import type { TablePluginItem } from "./types";

type McpEntry = NonNullable<McpListResult["entries"]>[number];

export function PluginTableRow({
  item,
  mcpEntry,
  busy,
  onEdit,
  onConfig,
  onStderr,
  onRemove,
  onPurge,
}: {
  item: TablePluginItem;
  mcpEntry?: McpEntry;
  busy: boolean;
  onEdit: (item: TablePluginItem) => void;
  onConfig: (item: TablePluginItem) => void;
  onStderr: (name: string) => void;
  onRemove: (name: string) => void;
  onPurge: (item: TablePluginItem) => void;
}) {
  return (
    <tr className="transition-colors hover:bg-muted/30">
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
          {/* ADR-0045:provider 声明的插件身份(真实 kind,非前端猜测) */}
          {item.pluginKind ? (
            <span
              className="shrink-0 rounded border border-border/60 bg-muted/40 px-1 font-mono text-[9.5px] text-muted-foreground"
              title={`插件身份: kind=${item.pluginKind}${item.pluginId ? ` · id=${item.pluginId}` : ""}${item.pluginVersion ? ` · v${item.pluginVersion}` : ""}`}
            >
              {item.pluginKind}
            </span>
          ) : null}
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
        ) : item.type === "wasm" ? (
          <Badge variant="outline" className="gap-1 border-violet-500/30 bg-violet-500/10 font-mono text-[10.5px] text-violet-600 dark:text-violet-400">
            <Boxes className="size-3" /> Wasm 插件
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
        {item.type === "wasm" ? (
          <div className="flex items-center justify-center gap-1">
            <Button
              variant="ghost"
              size="sm"
              disabled={busy}
              className="h-7 px-2 text-[11.5px] text-destructive hover:bg-destructive/10"
              title="移除该 wasm 插件声明并即时下线(热重载,无需重启)"
              data-slot="wasm-remove"
              onClick={() => onRemove(item.name)}
            >
              卸载
            </Button>
          </div>
        ) : item.type === "external" && item.serverRef ? (
          <div className="flex items-center justify-center gap-1">
            <Button
              variant="ghost"
              size="sm"
              className="h-7 px-2 text-[11.5px]"
              onClick={() => onEdit(item)}
            >
              编辑
            </Button>
            {mcpEntry?.manifest?.config_schema?.length ? (
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-[11.5px]"
                onClick={() => onConfig(item)}
              >
                配置
              </Button>
            ) : null}
            <Button
              variant="ghost"
              size="sm"
              className="h-7 px-2 text-[11.5px]"
              title="子进程 stderr 尾部(环形缓冲,跨重启带代标记)"
              data-slot="mcp-stderr"
              onClick={() => onStderr(item.name)}
            >
              日志
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={busy}
              className="h-7 px-2 text-[11.5px]"
              onClick={() => onRemove(item.name)}
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
              onClick={() => onPurge(item)}
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
}
