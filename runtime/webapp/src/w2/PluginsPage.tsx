// W2 设置中心 · 插件页。对象语义(2026-09-01 裁决,登记 W2 规格 §1.2):
// 插件 = 运行时能力提供方——系统内置能力(编译期注册,禁卸载)+
// MCP 服务器组(卸载 = 移出 MCP 配置文件,重启生效)。PIN = 壳子本地
// 偏好(localStorage),PIN 后在设置左侧导航快捷显示。
import { useCallback, useEffect, useMemo, useState } from "react";
import { Loader2Icon, PinIcon, PinOffIcon, Settings2Icon, Trash2Icon } from "lucide-react";
import { api, type Capability } from "./api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

const PIN_KEY = "bm_plugin_pins";

export function readPins(): string[] {
  try {
    const raw = localStorage.getItem(PIN_KEY);
    return raw ? (JSON.parse(raw) as string[]) : [];
  } catch {
    return [];
  }
}

export function writePins(pins: string[]) {
  localStorage.setItem(PIN_KEY, JSON.stringify(pins));
}

export type PluginEntry = {
  key: string; // 稳定标识:builtin:name | mcp:name
  name: string;
  category: "系统内置" | "MCP";
  detail: string;
  uninstallable: boolean;
};

export function PluginsPage({
  initialFilter,
  onGoMcpEdit,
}: {
  initialFilter?: string;
  onGoMcpEdit?: (name: string) => void;
}) {
  const [filter, setFilter] = useState(initialFilter ?? "");
  const [builtin, setBuiltin] = useState<Capability[]>([]);
  const [mcp, setMcp] = useState<
    { name: string; tools: number | null; loaded: boolean; pendingRemoval?: boolean }[]
  >([]);
  const [pins, setPins] = useState<string[]>(() => readPins());
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const r = await api.capabilities();
      setBuiltin(r.builtin);
      setMcp(r.mcp);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => {
    if (initialFilter) setFilter(initialFilter);
  }, [initialFilter]);

  const entries: PluginEntry[] = useMemo(() => {
    const out: PluginEntry[] = builtin.map((c) => ({
      key: `builtin:${c.name}`,
      name: c.name,
      category: "系统内置",
      detail: `${c.effect ?? ""}${c.idempotent ? " · 幂等" : ""}${
        c.approval && c.approval !== "not-required" ? " · 需审批" : ""
      }`,
      uninstallable: false,
    }));
    for (const m of mcp) {
      const detail = m.pendingRemoval
        ? `${m.tools ?? "?"} 个工具 · 已移出配置,重启后消失`
        : m.loaded
          ? `${m.tools ?? "?"} 个工具 · stdio`
          : `stdio · 未装载(重启后生效)`;
      out.push({
        key: `mcp:${m.name}`,
        name: m.name,
        category: "MCP",
        detail,
        uninstallable: !m.pendingRemoval,
      });
    }
    return out;
  }, [builtin, mcp]);

  const shown = entries.filter((e) =>
    e.name.toLowerCase().includes(filter.trim().toLowerCase()),
  );

  const togglePin = (key: string) => {
    const next = pins.includes(key) ? pins.filter((p) => p !== key) : [...pins, key];
    setPins(next);
    writePins(next);
    // 通知设置页导航刷新 PIN 快捷项
    window.dispatchEvent(new CustomEvent("bm-pins-changed"));
  };

  const uninstall = async (name: string) => {
    if (!window.confirm(`卸载 MCP server「${name}」?(移出配置文件,重启生效)`)) return;
    setBusy(`mcp:${name}`);
    setError(null);
    try {
      await api.mcp.remove(name);
      await reload();
      setNotice(`已移出 MCP 配置,重启服务器后生效`);
      setTimeout(() => setNotice(null), 6000);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h2 className="text-[15px] font-semibold">插件</h2>
        <p className="text-muted-foreground text-[12.5px]">
          运行时能力提供方:系统内置能力不可卸载;MCP 项卸载 = 移出配置,重启生效。
          PIN 后在设置导航快捷显示。
        </p>
      </div>

      <Input
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        placeholder="按名称筛选插件…"
        data-slot="plugin-filter"
        className="max-w-xs"
      />

      {notice ? (
        <div className="rounded-lg border border-emerald-300 bg-emerald-50 px-3 py-2 text-[12.5px] text-emerald-700">
          {notice}
        </div>
      ) : null}
      {error ? (
        <div className="rounded-lg border border-red-300 bg-red-50 px-3 py-2 text-[12.5px] text-red-700">
          {error}
        </div>
      ) : null}

      <div className="flex flex-col gap-2">
        {shown.map((e) => {
          const pinned = pins.includes(e.key);
          return (
            <div
              key={e.key}
              className="bg-card flex items-center gap-2 rounded-xl border p-3"
              data-plugin={e.name}
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate font-mono text-[13px]">{e.name}</span>
                  <Badge
                    variant={e.category === "系统内置" ? "secondary" : "outline"}
                    className="text-[10px]"
                  >
                    {e.category}
                  </Badge>
                  {pinned ? (
                    <Badge className="text-[10px]">已 PIN</Badge>
                  ) : null}
                </div>
                <div className="text-muted-foreground mt-0.5 text-[12px]">
                  {e.detail}
                </div>
              </div>
              <Button
                variant="ghost"
                size="sm"
                title={pinned ? "取消 PIN" : "PIN 到设置导航"}
                onClick={() => togglePin(e.key)}
              >
                {pinned ? (
                  <PinIcon className="text-blue-600" />
                ) : (
                  <PinOffIcon />
                )}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                disabled={e.category !== "MCP"}
                title={
                  e.category === "MCP"
                    ? "编辑 MCP 接入配置"
                    : "系统内置能力无独立设置"
                }
                onClick={() => onGoMcpEdit?.(e.name)}
              >
                <Settings2Icon />
                设置
              </Button>
              <Button
                variant="ghost"
                size="sm"
                className="text-red-600 hover:text-red-700"
                disabled={!e.uninstallable || busy !== null}
                title={
                  e.uninstallable
                    ? "移出 MCP 配置(重启生效)"
                    : "系统与内置类不允许卸载"
                }
                data-slot="uninstall"
                onClick={() => void uninstall(e.name)}
              >
                {busy === `mcp:${e.name}` ? (
                  <Loader2Icon className="animate-spin" />
                ) : (
                  <Trash2Icon />
                )}
                卸载
              </Button>
            </div>
          );
        })}
        {shown.length === 0 ? (
          <div
            className={cn(
              "text-muted-foreground rounded-lg border border-dashed px-3 py-8 text-center text-[12.5px]",
            )}
          >
            没有匹配「{filter}」的插件。
          </div>
        ) : null}
      </div>
    </div>
  );
}
