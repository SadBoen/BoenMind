/**
 * 主面板：内容由 lib/navigation.tsx 注册表驱动（聊天窗口 / 设置页 / 占位）。
 */
import { useTranslation } from "react-i18next";
import { NAV } from "@/lib/navigation";
import { useAppStore } from "@/stores/app-store";

export function MainPanel() {
  const { t } = useTranslation();
  const activeNav = useAppStore((s) => s.activeNav);

  const entry = NAV[activeNav];
  const Main = entry.main;
  if (!Main) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        {t("common.comingSoon")}
      </div>
    );
  }
  return <Main />;
}
