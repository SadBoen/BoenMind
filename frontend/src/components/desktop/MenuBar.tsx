/**
 * 顶部菜单栏（macOS menu bar 风格）：左侧 = 开始按钮（小星星）+ 聚焦应用名；
 * 右侧 = 后端状态点 + 时钟。毛玻璃深色底，任何壁纸下对比度都足够。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles } from "lucide-react";
import { APPS } from "@/lib/app-registry";
import { useAppStore } from "@/stores/app-store";
import { cn } from "@/lib/utils";

export function MenuBar({
  startOpen,
  onToggleStart,
}: {
  startOpen: boolean;
  onToggleStart: () => void;
}) {
  const { t } = useTranslation();
  const focusedApp = useAppStore((s) => s.focusedApp);
  const online = useAppStore((s) => s.online);
  const [now, setNow] = useState(() => new Date());

  // 时钟：每 30s 刷新
  useEffect(() => {
    const timer = setInterval(() => setNow(new Date()), 30_000);
    return () => clearInterval(timer);
  }, []);

  const time = now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const focusedName = focusedApp ? t(APPS[focusedApp].nameKey) : "BoenMind";

  return (
    <header className="relative z-40 flex h-9 shrink-0 items-center gap-3 border-b border-black/10 bg-white/60 px-2 backdrop-blur-2xl dark:border-white/10 dark:bg-black/50">
      {/* 开始按钮（小星星）：菜单从按钮正下方下拉 */}
      <button
        type="button"
        aria-label={t("desktop.startMenu")}
        onClick={onToggleStart}
        className={cn(
          "flex h-7 w-7 items-center justify-center rounded-lg transition-colors",
          startOpen ? "bg-black/10 dark:bg-white/15" : "hover:bg-black/5 dark:hover:bg-white/10",
        )}
      >
        <Sparkles size={15} className="text-indigo-500 dark:text-indigo-300" />
      </button>

      {/* 聚焦应用名（macOS 菜单栏惯例） */}
      <span className="truncate text-[13px] font-medium text-foreground/80">{focusedName}</span>

      {/* 右侧：后端状态 + 时钟 */}
      <div className="ml-auto flex shrink-0 items-center gap-2 px-2 text-xs text-foreground/70">
        <span
          aria-label={online ? t("statusbar.backendOnline") : t("statusbar.backendOffline")}
          className={cn("h-2 w-2 rounded-full", online ? "bg-emerald-500" : "bg-red-500")}
        />
        <time className="tabular-nums">{time}</time>
      </div>
    </header>
  );
}
