/**
 * 通用「管理列表」：加载/空态/条目卡片（图标 + 徽标 + 启停 + 卸载 + 附加操作）。
 * 插件与 skill 设置页共用，消除"列表 + 启停 + 卸载"的模板重复。
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Plus, RefreshCw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";

export interface ManagedItem {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
}

interface Props<T extends ManagedItem> {
  items: T[];
  loading: boolean;
  /** 条目卡片主图标 */
  icon: ReactNode;
  /** 名称后的徽标列表（builtin / 来源等） */
  badges?: (item: T) => ReactNode[];
  /** 卡片右侧附加操作（设置按钮等，插在启停开关之前） */
  extraActions?: (item: T) => ReactNode;
  toggle: (item: T) => void;
  uninstall: (item: T) => void;
  refresh: () => void;
  /** 空列表文案 i18n key */
  emptyKey: string;
  /** 卸载按钮 title i18n key */
  uninstallTitleKey: string;
}

export function ManagedItemsList<T extends ManagedItem>({
  items,
  loading,
  icon,
  badges,
  extraActions,
  toggle,
  uninstall,
  emptyKey,
  uninstallTitleKey,
}: Props<T>) {
  const { t } = useTranslation();

  return loading ? (
    <p className="text-sm text-muted-foreground">{t("common.loading")}</p>
  ) : items.length === 0 ? (
    <div className="rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground">
      {t(emptyKey)}
    </div>
  ) : (
    <div className="space-y-3">
      {items.map((item) => (
        <div key={item.id} className="flex items-center justify-between gap-3 rounded-xl border p-4">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              {icon}
              <h3 className="font-semibold">{item.name}</h3>
              {badges?.(item)}
            </div>
            <p className="mt-1 truncate text-xs text-muted-foreground">{item.description}</p>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            {extraActions?.(item)}
            <Switch checked={item.enabled} onCheckedChange={() => toggle(item)} />
            <Button
              variant="ghost"
              size="icon"
              className="text-muted-foreground hover:text-destructive"
              title={t(uninstallTitleKey)}
              onClick={() => uninstall(item)}
            >
              <Trash2 size={15} />
            </Button>
          </div>
        </div>
      ))}
    </div>
  );
}

/** 本地安装输入行（插件/skill 共用）：路径输入 + 安装按钮 + 刷新按钮 */
export function LocalInstallRow({
  placeholderKey,
  value,
  onChange,
  onInstall,
  onRefresh,
  refreshing,
}: {
  placeholderKey: string;
  value: string;
  onChange: (v: string) => void;
  onInstall: () => void;
  onRefresh: () => void;
  refreshing: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex gap-2">
      <div className="relative flex-1">
        <Plus size={15} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" />
        <Input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={t(placeholderKey)}
          className="pl-8 font-mono text-xs"
        />
      </div>
      <Button variant="outline" onClick={onInstall}>
        {t("settings.common.install")}
      </Button>
      <Button variant="ghost" size="icon" onClick={onRefresh} title={t("common.refresh")}>
        <RefreshCw size={15} className={refreshing ? "animate-spin" : ""} />
      </Button>
    </div>
  );
}
