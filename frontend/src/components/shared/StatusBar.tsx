/**
 * 底部状态栏：后端状态点+版本 | 模型 | 工作文件夹。
 * （原桌面壳 StatusBar 的 classic 变体；桌面形态退役后仅剩此变体，2026-08-16）
 */
import { useTranslation } from "react-i18next";
import { FolderOpen, HardDrive, Wifi, WifiOff } from "lucide-react";
import { useAppStore } from "@/stores/app-store";
import { cn } from "@/lib/utils";

export function StatusBar() {
  const { t } = useTranslation();
  const online = useAppStore((s) => s.online);
  const health = useAppStore((s) => s.health);
  const config = useAppStore((s) => s.config);

  const model = config?.default_model ?? t("statusbar.noModel");

  return (
    <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-border bg-muted/40 px-3 text-[11px] text-muted-foreground">
      <span className="flex items-center gap-1.5">
        {online ? (
          <Wifi size={12} className="text-emerald-400" />
        ) : (
          <WifiOff size={12} className="text-red-400" />
        )}
        <span className={cn(!online && "font-medium text-red-400")}>
          {online
            ? t("statusbar.backendV", { version: health?.version ?? "" })
            : t("statusbar.backendOffline")}
        </span>
      </span>

      <span className="hidden items-center gap-1.5 sm:flex">
        <HardDrive size={12} />
        {model}
      </span>

      <span className="hidden min-w-0 flex-1 items-center gap-1.5 truncate md:flex">
        <FolderOpen size={12} />
        <span className="truncate" title={config?.working_dir}>
          {config?.working_dir ?? t("statusbar.loadingDir")}
        </span>
      </span>

      <span className="ml-auto shrink-0">BoenMind</span>
    </footer>
  );
}
