/**
 * 插件设置：列表 + 启停开关 + 安装（本地路径 / npm:/git: 包源）+ 权限模式（安全/宽松/YOLO）
 * + 设置页（schema 驱动动态表单）。
 * 插件基于 pi 扩展机制（QuickJS 直接加载 TypeScript），无需转 Rust。
 * 列表/安装行复用 ManagedItemsList（与 skill 设置页同构）。
 */
import { useEffect, useState } from "react";
import { useTranslation, Trans } from "react-i18next";
import { LayoutGrid, Puzzle, Settings2, ShieldCheck } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";
import { api, type PluginInfo } from "@/api/client";
import { LocalInstallRow, ManagedItemsList } from "./ManagedItemsList";
import { PluginSettingsDialog } from "./PluginSettingsDialog";
import { ScopeBadge, ScopePicker } from "./ScopePicker";
import { useAppStore } from "@/stores/app-store";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cn } from "@/lib/utils";

/** 权限模式 → 配置值：default（不设置，走上游默认）/ safe / balanced / yolo（permissive + 危险放行） */
const PERMISSION_MODES = ["default", "safe", "balanced", "yolo"] as const;

/** 分类筛选：all = 全部 / system = 系统增强 / app = 功能插件（manifest category 标签） */
type CategoryFilter = "all" | "system" | "app";
const CATEGORY_FILTERS: CategoryFilter[] = ["all", "system", "app"];

export function PluginsSettings() {
  const { t } = useTranslation();
  const config = useAppStore((s) => s.config);
  const tierExpert = useAppStore((s) => s.settingsTier) === "expert";
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [installPath, setInstallPath] = useState("");
  const [settingsFor, setSettingsFor] = useState<PluginInfo | null>(null);
  const [permissionMode, setPermissionMode] = useState<string>("default");
  const [category, setCategory] = useState<CategoryFilter>("all");

  const load = async () => {
    setLoading(true);
    try {
      setPlugins(await api.listPlugins());
    } catch (err) {
      toast.error(t("settings.plugins.loadFailed", { error: String(err) }));
    } finally {
      setLoading(false);
    }
  };

  /** 读取权限模式（yolo = permissive + allowDangerous 的组合） */
  const loadPermissionMode = async () => {
    try {
      const cfg = await api.getConfig();
      const policy = cfg.extension_policy;
      const mode =
        policy === "permissive" && cfg.extension_allow_dangerous
          ? "yolo"
          : policy && PERMISSION_MODES.includes(policy as (typeof PERMISSION_MODES)[number])
            ? policy
            : "default";
      setPermissionMode(mode);
    } catch {
      // 读取失败保持默认展示，设置保存时会报错提示
    }
  };

  useEffect(() => {
    void load();
    void loadPermissionMode();
  }, []);

  /** 切换权限模式：读当前配置 → 改字段 → 全量保存 */
  const changePermissionMode = async (mode: string | null) => {
    if (!mode) return;
    const prev = permissionMode;
    setPermissionMode(mode);
    try {
      const cfg = await api.getConfig();
      cfg.extension_policy =
        mode === "default" || mode === "yolo" ? (mode === "yolo" ? "permissive" : undefined) : mode;
      cfg.extension_allow_dangerous = mode === "yolo";
      await api.saveConfig(cfg);
      toast.success(t("settings.plugins.permissionModeSaved"));
    } catch (err) {
      toast.error(t("settings.plugins.permissionModeSaveFailed", { error: String(err) }));
      setPermissionMode(prev);
    }
  };

  const toggle = async (plugin: PluginInfo) => {
    try {
      await api.setPlugin(plugin.id, !plugin.enabled);
      toast.success(
        plugin.enabled
          ? t("settings.plugins.toggledOff", { name: plugin.name })
          : t("settings.plugins.toggledOn", { name: plugin.name }),
      );
      await load();
    } catch (err) {
      toast.error(t("settings.plugins.toggleFailed", { error: String(err) }));
    }
  };

  const install = async () => {
    const raw = installPath.trim();
    if (!raw) {
      toast.error(t("settings.plugins.pathRequired"));
      return;
    }
    try {
      // npm:/git: 前缀走包源安装（上游包管理器），否则按本地路径安装
      if (/^(npm:|git:)/.test(raw)) {
        await api.installPluginFromSource(raw);
      } else {
        await api.installPlugin(raw);
      }
      toast.success(t("settings.plugins.installed"));
      setInstallPath("");
      await load();
    } catch (err) {
      toast.error(t("settings.plugins.installFailed", { error: String(err) }));
    }
  };

  const uninstall = async (plugin: PluginInfo) => {
    // 内置插件卸载后不再自动恢复，需二次确认
    if (
      plugin.builtin &&
      !window.confirm(t("settings.plugins.uninstallBuiltinConfirm", { name: plugin.name }))
    ) {
      return;
    }
    try {
      await api.uninstallPlugin(plugin.id);
      toast.success(t("settings.plugins.uninstalled", { name: plugin.name }));
      await load();
    } catch (err) {
      toast.error(t("settings.plugins.uninstallFailed", { error: String(err) }));
    }
  };

  return (
    <section className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">{t("settings.plugins.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("settings.plugins.desc")}</p>
      </div>

      {/* 分类标签：全部 / 系统增强 / 功能插件（manifest category 声明驱动） */}
      <div
        role="tablist"
        aria-label={t("settings.plugins.category.label")}
        className="flex w-fit items-center gap-1 rounded-lg bg-muted/60 p-1"
      >
        {CATEGORY_FILTERS.map((c) => {
          const count =
            c === "all" ? plugins.length : plugins.filter((p) => (p.category ?? "system") === c).length;
          return (
            <button
              key={c}
              type="button"
              role="tab"
              aria-selected={category === c}
              onClick={() => setCategory(c)}
              className={cn(
                "flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium transition-colors",
                category === c
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {c === "all" && <LayoutGrid size={12} />}
              {t(`settings.plugins.category.${c}`)}
              <span className="text-[10px] tabular-nums text-muted-foreground/70">{count}</span>
            </button>
          );
        })}
      </div>

      {/* 插件权限模式：安全（询问关键能力）/ 宽松 / YOLO（全自动放行，含危险能力） */}
      <div className="flex items-center justify-between gap-3 rounded-xl border p-4">
        <div className="flex min-w-0 items-center gap-2">
          <ShieldCheck size={15} className="shrink-0 text-muted-foreground" />
          <div className="min-w-0">
            <h3 className="text-sm font-medium">{t("settings.plugins.permissionMode.title")}</h3>
            <p className="text-xs text-muted-foreground">{t("settings.plugins.permissionMode.desc")}</p>
          </div>
        </div>
        <Select
          value={permissionMode}
          onValueChange={(v) => void changePermissionMode(v)}
          itemToStringLabel={(v) => t(`settings.plugins.permissionMode.options.${v}`)}
        >
          <SelectTrigger className="w-40 shrink-0">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PERMISSION_MODES.map((m) => (
              <SelectItem key={m} value={m}>
                {t(`settings.plugins.permissionMode.options.${m}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <LocalInstallRow
        placeholderKey="settings.plugins.installPlaceholder"
        value={installPath}
        onChange={setInstallPath}
        onInstall={() => void install()}
        onRefresh={() => void load()}
        refreshing={loading}
      />

      <ManagedItemsList
        items={category === "all" ? plugins : plugins.filter((p) => (p.category ?? "system") === category)}
        loading={loading}
        icon={<Puzzle size={15} className="shrink-0 text-muted-foreground" />}
        badges={(plugin) => [
          ...(plugin.builtin
            ? [
                <Badge key="builtin" variant="secondary" className="text-[10px]">
                  {t("settings.plugins.builtin")}
                </Badge>,
              ]
            : []),
          <Badge key="kind" variant="outline" className="text-[10px] font-normal">
            {plugin.kind === "single" ? t("settings.plugins.singleFile") : t("settings.plugins.manifestDir")}
          </Badge>,
          <ScopeBadge key="scope" scopes={config?.plugin_scopes?.[plugin.id]} />,
        ]}
        extraActions={(plugin) => (
          <>
            {tierExpert && (
              <ScopePicker
                key="scope"
                name={plugin.name}
                current={config?.plugin_scopes?.[plugin.id]}
                onSave={async (scopes) => {
                  await api.setPluginScope(plugin.id, scopes);
                  await useAppStore.getState().loadConfig();
                }}
              />
            )}
            {plugin.settingsSchema ? (
              <Button
                variant="ghost"
                size="icon"
                className="text-muted-foreground"
                title={t("settings.plugins.settings")}
                onClick={() => setSettingsFor(plugin)}
              >
                <Settings2 size={15} />
              </Button>
            ) : null}
          </>
        )}
        toggle={(plugin) => void toggle(plugin)}
        uninstall={(plugin) => void uninstall(plugin)}
        emptyKey="settings.plugins.empty"
        uninstallTitleKey="settings.plugins.uninstall"
      />

      <p className="text-xs text-muted-foreground">
        <Trans
          i18nKey="settings.plugins.tip"
          components={{ code: <code className="rounded bg-muted px-1" /> }}
        />
      </p>

      {/* 插件设置页（schema 驱动动态表单） */}
      {settingsFor && (
        <PluginSettingsDialog
          plugin={settingsFor}
          open={settingsFor !== null}
          onClose={() => setSettingsFor(null)}
        />
      )}
    </section>
  );
}
