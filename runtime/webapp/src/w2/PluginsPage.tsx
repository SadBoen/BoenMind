// W2 设置中心 · 插件页。对象语义(2026-09-02 用户裁决改版):插件页只列
// **系统内置能力**(编译期注册,禁卸载);MCP 服务器组不再在此重复展示,
// 统一归「MCP 管理」页(此前同页双列 MCP 项与 MCP 管理重复,用户指摘)。
// PIN = 壳子本地偏好(localStorage),PIN 后在设置左侧导航快捷显示;
// 旧数据里的 mcp: 前缀 PIN 由 SettingsPage 兼容路由到 MCP 管理页。
import { useCallback, useEffect, useMemo, useState } from "react";
import { PinIcon, PinOffIcon } from "lucide-react";
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
  key: string; // 稳定标识:builtin:name(mcp: 旧 PIN 数据仍兼容)
  name: string;
  category: "系统内置" | "MCP";
  detail: string;
};

export function PluginsPage({ initialFilter }: { initialFilter?: string }) {
  const [filter, setFilter] = useState(initialFilter ?? "");
  const [builtin, setBuiltin] = useState<Capability[]>([]);
  const [pins, setPins] = useState<string[]>(() => readPins());
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const r = await api.capabilities();
      setBuiltin(r.builtin);
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

  const entries: PluginEntry[] = useMemo(
    () =>
      builtin.map((c) => ({
        key: `builtin:${c.name}`,
        name: c.name,
        category: "系统内置" as const,
        detail: `${c.effect ?? ""}${c.idempotent ? " · 幂等" : ""}${
          c.approval && c.approval !== "not-required" ? " · 需审批" : ""
        }`,
      })),
    [builtin],
  );

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

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h2 className="text-[15px] font-semibold">插件</h2>
        <p className="text-muted-foreground text-[12.5px]">
          系统内置能力(编译期注册,禁卸载)。MCP 服务器的查看/接入/卸载统一在
          「MCP 管理」页。
        </p>
      </div>

      <Input
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        placeholder="按名称筛选插件…"
        data-slot="plugin-filter"
        className="max-w-xs"
      />

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
                  <Badge variant="secondary" className="text-[10px]">
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
