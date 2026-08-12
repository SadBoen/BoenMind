/**
 * 设置二级菜单：条目由 lib/navigation.tsx 的 SETTINGS 注册表驱动
 * （新增设置页只需在注册表加一行，菜单与主面板自动联动）。
 */
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { SETTINGS } from "@/lib/navigation";
import { useAppStore } from "@/stores/app-store";

export function SettingsMenu() {
  const { t } = useTranslation();
  const settingsTab = useAppStore((s) => s.settingsTab);
  const setSettingsTab = useAppStore((s) => s.setSettingsTab);

  return (
    <div className="flex flex-col gap-0.5 p-2">
      {Object.entries(SETTINGS).map(([key, item]) => (
        <button
          key={key}
          type="button"
          onClick={() => setSettingsTab(key as typeof settingsTab)}
          className={cn(
            "flex items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm transition-colors",
            settingsTab === key
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
