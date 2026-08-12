/**
 * 插件设置：列表 + 启停开关 + 本地安装 + 设置页（schema 驱动动态表单）。
 * 插件基于 pi 扩展机制（QuickJS 直接加载 TypeScript），无需转 Rust。
 * 列表/安装行复用 ManagedItemsList（与 skill 设置页同构）。
 */
import { useEffect, useState } from "react";
import { useTranslation, Trans } from "react-i18next";
import { Puzzle, Settings2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";
import { api, type PluginInfo } from "@/api/client";
import { LocalInstallRow, ManagedItemsList } from "./ManagedItemsList";
import { PluginSettingsDialog } from "./PluginSettingsDialog";

export function PluginsSettings() {
  const { t } = useTranslation();
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [installPath, setInstallPath] = useState("");
  const [settingsFor, setSettingsFor] = useState<PluginInfo | null>(null);

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

  useEffect(() => {
    void load();
  }, []);

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
    const path = installPath.trim();
    if (!path) {
      toast.error(t("settings.plugins.pathRequired"));
      return;
    }
    try {
      await api.installPlugin(path);
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

      <LocalInstallRow
        placeholderKey="settings.plugins.installPlaceholder"
        value={installPath}
        onChange={setInstallPath}
        onInstall={() => void install()}
        onRefresh={() => void load()}
        refreshing={loading}
      />

      <ManagedItemsList
        items={plugins}
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
        ]}
        extraActions={(plugin) =>
          plugin.settingsSchema ? (
            <Button
              variant="ghost"
              size="icon"
              className="text-muted-foreground"
              title={t("settings.plugins.settings")}
              onClick={() => setSettingsFor(plugin)}
            >
              <Settings2 size={15} />
            </Button>
          ) : null
        }
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
