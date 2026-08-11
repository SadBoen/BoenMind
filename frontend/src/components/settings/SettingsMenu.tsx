/**
 * 设置二级菜单：外观 / 模型提供商 / 工作文件夹。
 */
import { useTranslation } from "react-i18next";
import { FolderOpen, Info, MonitorCog, Palette, Puzzle, Wand2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAppStore, type SettingsTab } from "@/stores/app-store";

const MENU_ITEMS: { key: SettingsTab; labelKey: string; icon: React.ReactNode; descKey: string }[] = [
  { key: "appearance", labelKey: "settings.menu.appearance", icon: <Palette size={16} />, descKey: "settings.menu.appearanceDesc" },
  { key: "providers", labelKey: "settings.menu.providers", icon: <MonitorCog size={16} />, descKey: "settings.menu.providersDesc" },
  { key: "workspace", labelKey: "settings.menu.workspace", icon: <FolderOpen size={16} />, descKey: "settings.menu.workspaceDesc" },
  { key: "plugins", labelKey: "settings.menu.plugins", icon: <Puzzle size={16} />, descKey: "settings.menu.pluginsDesc" },
  { key: "skills", labelKey: "settings.menu.skills", icon: <Wand2 size={16} />, descKey: "settings.menu.skillsDesc" },
  { key: "about", labelKey: "settings.menu.about", icon: <Info size={16} />, descKey: "settings.menu.aboutDesc" },
];

export function SettingsMenu() {
  const { t } = useTranslation();
  const settingsTab = useAppStore((s) => s.settingsTab);
  const setSettingsTab = useAppStore((s) => s.setSettingsTab);

  return (
    <div className="flex flex-col gap-0.5 p-2">
      {MENU_ITEMS.map((item) => (
        <button
          key={item.key}
          type="button"
          onClick={() => setSettingsTab(item.key)}
          className={cn(
            "flex items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm transition-colors",
            settingsTab === item.key
              ? "bg-accent text-accent-foreground"
              : "hover:bg-accent/50",
          )}
        >
          {item.icon}
          <div className="min-w-0">
            <p className="font-medium leading-tight">{t(item.labelKey)}</p>
            <p className="text-[10px] text-muted-foreground">{t(item.descKey)}</p>
          </div>
        </button>
      ))}
    </div>
  );
}
