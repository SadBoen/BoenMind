/**
 * 底部状态栏：横跨所有区域。后端连接状态 / 当前模型 / 工作目录。
 */
import { useTranslation } from "react-i18next";
import { FolderOpen, HardDrive, Wifi, WifiOff } from "lucide-react";
import { useAppStore } from "@/stores/app-store";

export function StatusBar() {
  const { t } = useTranslation();
  const online = useAppStore((s) => s.online);
  const health = useAppStore((s) => s.health);
  const config = useAppStore((s) => s.config);

  const model = config?.default_model ?? t("statusbar.noModel");

  return (
    <footer className="flex h-7 shrink-0 items-center gap-4 border-t bg-muted/30 px-3 text-[11px] text-muted-foreground">
      <span className="flex items-center gap-1.5">
        {online ? (
          <Wifi size={12} className="text-emerald-500" />
        ) : (
          <WifiOff size={12} className="text-destructive" />
        )}
        <span className={online ? "" : "font-medium text-destructive"}>
          {online ? t("statusbar.backendV", { version: health?.version ?? "" }) : t("statusbar.backendOffline")}
        </span>
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
  );
}
