/**
 * 设置二级菜单：条目由 lib/app-registry.tsx 的 SETTINGS 注册表驱动
 * （新增设置页只需在注册表加一行，菜单与主面板自动联动）。
 * 分组（设置架构 2026-08-16）：应用组（每软件 APP 设置）+ 系统组（全局设置）。
 */
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { SETTINGS, type SettingsTab } from "@/lib/app-registry";
import { useAppStore } from "@/stores/app-store";

const GROUP_ORDER: ("app" | "system")[] = ["app", "system"];

export function SettingsMenu() {
  const { t } = useTranslation();
  const settingsTab = useAppStore((s) => s.settingsTab);
  const setSettingsTab = useAppStore((s) => s.setSettingsTab);
  const settingsTier = useAppStore((s) => s.settingsTier);

  const renderItem = (key: string, item: (typeof SETTINGS)[SettingsTab]) => (
    <button
      key={key}
      type="button"
      onClick={() => setSettingsTab(key as SettingsTab)}
      className={cn(
        "flex items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm transition-colors",
        settingsTab === key ? "bg-accent text-accent-foreground" : "hover:bg-accent/50",
      )}
    >
      {item.icon}
      <div className="min-w-0">
        <p className="font-medium leading-tight">{t(item.labelKey)}</p>
        <p className="text-[10px] text-muted-foreground">{t(item.descKey)}</p>
      </div>
    </button>
  );

  return (
    <div className="flex flex-col gap-1 p-2">
      {GROUP_ORDER.map((group) => {
        const items = Object.entries(SETTINGS).filter(
          ([, item]) =>
            (item.group ?? "system") === group &&
            // 普通模式隐藏资深项（设置架构 §十：切换只改可见性，不动设置值）
            (settingsTier === "expert" || item.tier !== "expert"),
        );
        if (items.length === 0) return null;
        return (
          <div key={group} className="flex flex-col gap-0.5">
            <p className="px-2.5 pb-0.5 pt-1 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
              {t(`settings.menu.group.${group}`)}
            </p>
            {items.map(([key, item]) => renderItem(key, item))}
          </div>
        );
      })}
    </div>
  );
}
