/**
 * Dock（macOS 风格）：底部居中的毛玻璃胶囊——5 个应用图标（Cosmos 渐变方块）。
 * 运行中的应用有白色指示点；点击：未开则打开，已开则聚焦置顶。
 * 悬停轻微放大（macOS Dock 的简化版）。
 */
import { useTranslation } from "react-i18next";
import { APP_LIST } from "@/lib/app-registry";
import { useAppStore } from "@/stores/app-store";
import { cn } from "@/lib/utils";

export function Dock() {
  const { t } = useTranslation();
  const openApps = useAppStore((s) => s.openApps);
  const focusedApp = useAppStore((s) => s.focusedApp);
  const openApp = useAppStore((s) => s.openApp);
  const focusApp = useAppStore((s) => s.focusApp);

  return (
    <nav
      aria-label={t("desktop.dock")}
      className="z-30 flex h-16 shrink-0 items-end justify-center pb-1.5"
    >
      <div className="flex items-end gap-1.5 rounded-2xl border border-white/20 bg-white/50 px-2 pb-1.5 pt-2 shadow-lg backdrop-blur-2xl dark:bg-black/50">
        {APP_LIST.map((app) => {
          const open = openApps.includes(app.id);
          const active = focusedApp === app.id;
          return (
            <button
              key={app.id}
              type="button"
              aria-label={t(app.nameKey)}
              onClick={() => (open ? focusApp(app.id) : openApp(app.id))}
              className="group relative flex flex-col items-center"
            >
              <span
                className={cn(
                  "flex h-10 w-10 items-center justify-center rounded-xl text-white shadow-md transition-all duration-150 group-hover:-translate-y-1.5 group-hover:scale-125",
                  active && "ring-2 ring-white/50",
                )}
                style={{ background: app.gradient }}
              >
                {app.icon}
              </span>
              {/* 运行指示点 */}
              <span
                className={cn(
                  "mt-1 h-1 w-1 rounded-full bg-foreground/60 transition-opacity",
                  open ? "opacity-100" : "opacity-0",
                )}
              />
            </button>
          );
        })}
      </div>
    </nav>
  );
}
