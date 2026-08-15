/**
 * 开始菜单：左下玻璃面板——彩色渐变应用卡网格（Cosmos 风）
 * + 底部脚注（版本/后端状态/工作文件夹，原 StatusBar 信息迁入处）。
 * 自绘层（不用 Base UI Dialog）：点外/Esc 关闭，避免 portal 脱离桌面层级。
 */
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, HardDrive, Wifi, WifiOff } from "lucide-react";
import { APP_LIST } from "@/lib/app-registry";
import { useAppStore } from "@/stores/app-store";
import { cn } from "@/lib/utils";

export function StartMenu({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const openApp = useAppStore((s) => s.openApp);
  const online = useAppStore((s) => s.online);
  const health = useAppStore((s) => s.health);
  const config = useAppStore((s) => s.config);

  // Esc 关闭
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const model = config?.default_model ?? t("statusbar.noModel");

  return (
    <>
      {/* 点外关闭：全屏透明层（低于菜单） */}
      <div className="fixed inset-0 z-40" onPointerDown={onClose} aria-hidden />
      <div className="absolute bottom-14 left-2 z-50 flex w-105 max-w-[calc(100vw-1rem)] flex-col gap-4 rounded-2xl border border-white/20 bg-white/70 p-5 shadow-2xl backdrop-blur-2xl dark:bg-black/60">
        {/* 应用卡网格 */}
        <div className="grid grid-cols-2 gap-3">
          {APP_LIST.map((app) => (
            <button
              key={app.id}
              type="button"
              onClick={() => {
                openApp(app.id);
                onClose();
              }}
              className="group flex items-center gap-3 rounded-2xl border border-white/10 bg-white/40 p-3 text-left transition-all hover:-translate-y-0.5 hover:bg-white/70 hover:shadow-lg dark:bg-white/5 dark:hover:bg-white/10"
            >
              <span
                className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl text-white shadow-md transition-transform group-hover:scale-110"
                style={{ background: app.gradient }}
              >
                {app.icon}
              </span>
              <span className="min-w-0">
                <span className="block truncate text-sm font-medium text-foreground">
                  {t(app.nameKey)}
                </span>
                <span className={cn("block text-[11px] text-muted-foreground")}>
                  {app.id === "chat" ? t("desktop.app.chatDesc") : app.id === "coding" ? t("desktop.app.codingDesc") : t("desktop.app.toolDesc")}
                </span>
              </span>
            </button>
          ))}
        </div>

        {/* 脚注：版本/后端状态/工作文件夹（原 StatusBar 信息） */}
        <footer className="flex flex-wrap items-center gap-x-4 gap-y-1 border-t border-white/15 pt-3 text-[11px] text-muted-foreground dark:border-white/10">
          <span className="flex items-center gap-1.5">
            {online ? (
              <Wifi size={12} className="text-emerald-500" />
            ) : (
              <WifiOff size={12} className="text-red-400" />
            )}
            {online
              ? t("statusbar.backendV", { version: health?.version ?? "" })
              : t("statusbar.backendOffline")}
          </span>
          <span className="hidden items-center gap-1.5 sm:flex">
            <HardDrive size={12} />
            {model}
          </span>
          <span className="hidden flex-1 items-center gap-1.5 truncate md:flex">
            <FolderOpen size={12} />
            <span className="truncate" title={config?.working_dir}>
              {config?.working_dir ?? t("statusbar.loadingDir")}
            </span>
          </span>
          <span className="ml-auto shrink-0">BoenMind</span>
        </footer>
      </div>
    </>
  );
}
