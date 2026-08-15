/**
 * 任务栏：底部 48px 毛玻璃——开始按钮、运行中的应用（开窗高亮/点击聚焦）、
 * 后端状态点 + 时钟（右置）。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles } from "lucide-react";
import { APPS } from "@/lib/app-registry";
import { useAppStore } from "@/stores/app-store";
import { cn } from "@/lib/utils";

export function Taskbar({
  startOpen,
  onToggleStart,
}: {
  startOpen: boolean;
  onToggleStart: () => void;
}) {
  const { t } = useTranslation();
  const openApps = useAppStore((s) => s.openApps);
  const focusedApp = useAppStore((s) => s.focusedApp);
  const openApp = useAppStore((s) => s.openApp);
  const focusApp = useAppStore((s) => s.focusApp);
  const online = useAppStore((s) => s.online);
  const [now, setNow] = useState(() => new Date());

  // 时钟：每分钟刷新
  useEffect(() => {
    const timer = setInterval(() => setNow(new Date()), 30_000);
    return () => clearInterval(timer);
  }, []);

  const time = now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

  return (
    <footer className="relative z-40 flex h-12 shrink-0 items-center gap-1 border-t border-white/10 bg-white/10 px-2 backdrop-blur-xl dark:bg-black/30">
      {/* 开始按钮 */}
      <button
        type="button"
        aria-label={t("desktop.startMenu")}
        onClick={onToggleStart}
        className={cn(
          "flex h-9 w-9 items-center justify-center rounded-xl transition-colors",
          startOpen ? "bg-white/25" : "hover:bg-white/15",
        )}
      >
        <Sparkles size={18} className="text-indigo-300 dark:text-indigo-200" />
      </button>

      {/* 运行中的应用：开窗高亮，点击聚焦/置顶 */}
      <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
        {openApps.map((id) => {
          const entry = APPS[id];
          if (!entry) return null;
          const active = focusedApp === id;
          return (
            <button
              key={id}
              type="button"
              aria-label={t(entry.nameKey)}
              onClick={() => (active ? openApp(id) : focusApp(id))}
              className={cn(
                "flex h-9 w-9 shrink-0 items-center justify-center rounded-xl transition-colors",
                active ? "bg-white/25" : "hover:bg-white/15",
              )}
            >
              <span
                className={cn("flex h-6 w-6 items-center justify-center rounded-lg text-white shadow-sm", active && "ring-2 ring-white/40")}
                style={{ background: entry.gradient }}
              >
                {entry.icon}
              </span>
            </button>
          );
        })}
      </div>

      {/* 后端状态点 + 时钟 */}
      <div className="flex shrink-0 items-center gap-2 px-2 text-xs text-white/80 dark:text-white/70">
        <span
          aria-label={online ? t("statusbar.backendOnline") : t("statusbar.backendOffline")}
          className={cn("h-2 w-2 rounded-full", online ? "bg-emerald-400" : "bg-red-400")}
        />
        <time className="tabular-nums">{time}</time>
      </div>
    </footer>
  );
}
