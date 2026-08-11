/**
 * 二级面板：随导航切换内容。
 * - 对话 → 会话列表
 * - 设置 → 设置二级菜单（外观 / 模型提供商 / 工作文件夹）
 */
import { useTranslation } from "react-i18next";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useAppStore, type NavKey } from "@/stores/app-store";
import { SessionList } from "@/components/chat/SessionList";
import { SettingsMenu } from "@/components/settings/SettingsMenu";

export function SecondaryPanel() {
  const { t } = useTranslation();
  const activeNav = useAppStore((s) => s.activeNav);

  return (
    <div className="flex h-full min-w-0 flex-col border-r bg-background">
      <PanelHeader nav={activeNav} />
      <ScrollArea className="min-h-0 flex-1">
        {activeNav === "chat" && <SessionList />}
        {activeNav === "settings" && <SettingsMenu />}
        {(activeNav === "gallery" || activeNav === "knowledge") && (
          <div className="p-4 text-sm text-muted-foreground">{t("common.comingSoon")}</div>
        )}
      </ScrollArea>
    </div>
  );
}

function PanelHeader({ nav }: { nav: NavKey }) {
  const { t } = useTranslation();
  const title =
    nav === "chat"
      ? t("nav.chat")
      : nav === "settings"
        ? t("nav.settings")
        : nav === "gallery"
          ? t("nav.gallery")
          : t("nav.knowledge");
  return (
    <div className="flex h-11 shrink-0 items-center border-b px-3">
      <h2 className="text-sm font-semibold">{title}</h2>
    </div>
  );
}
