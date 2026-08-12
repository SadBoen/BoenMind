/**
 * 二级面板：内容由 lib/navigation.tsx 注册表驱动（会话列表 / 设置菜单 / 占位）。
 */
import { useTranslation } from "react-i18next";
import { ScrollArea } from "@/components/ui/scroll-area";
import { NAV } from "@/lib/navigation";
import { useAppStore } from "@/stores/app-store";

export function SecondaryPanel() {
  const { t } = useTranslation();
  const activeNav = useAppStore((s) => s.activeNav);

  const entry = NAV[activeNav];
  const Secondary = entry.secondary;

  return (
    <div className="flex h-full min-w-0 flex-col border-r bg-background">
      <PanelHeader title={t(entry.labelKey)} />
      <ScrollArea className="min-h-0 flex-1">
        {Secondary ? (
          <Secondary />
        ) : (
          <div className="p-4 text-sm text-muted-foreground">{t("common.comingSoon")}</div>
        )}
      </ScrollArea>
    </div>
  );
}

function PanelHeader({ title }: { title: string }) {
  return (
    <div className="flex h-11 shrink-0 items-center border-b px-3">
      <h2 className="text-sm font-semibold">{title}</h2>
    </div>
  );
}
