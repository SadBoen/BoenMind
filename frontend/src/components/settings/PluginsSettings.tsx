/**
 * 插件设置：列表 + 启停开关 + 本地安装 + 设置页（schema 驱动动态表单）。
 * 插件基于 pi 扩展机制（QuickJS 直接加载 TypeScript），无需转 Rust。
 */
import { useEffect, useState } from "react";
import { useTranslation, Trans } from "react-i18next";
import { Puzzle, Plus, RefreshCw, Settings2, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { toast } from "sonner";
import { api, type PluginInfo } from "@/api/client";
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

      {/* 安装 */}
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Plus size={15} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={installPath}
            onChange={(e) => setInstallPath(e.target.value)}
            placeholder={t("settings.plugins.installPlaceholder")}
            className="pl-8 font-mono text-xs"
          />
        </div>
        <Button variant="outline" onClick={() => void install()}>
          {t("settings.plugins.install")}
        </Button>
        <Button variant="ghost" size="icon" onClick={() => void load()} title={t("common.refresh")}>
          <RefreshCw size={15} className={loading ? "animate-spin" : ""} />
        </Button>
      </div>

      {/* 列表 */}
      {loading ? (
        <p className="text-sm text-muted-foreground">{t("common.loading")}</p>
      ) : plugins.length === 0 ? (
        <div className="rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground">
          {t("settings.plugins.empty")}
        </div>
      ) : (
        <div className="space-y-3">
          {plugins.map((plugin) => (
            <div key={plugin.id} className="flex items-center justify-between gap-3 rounded-xl border p-4">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <Puzzle size={15} className="shrink-0 text-muted-foreground" />
                  <h3 className="font-semibold">{plugin.name}</h3>
                  {plugin.builtin && (
                    <Badge variant="secondary" className="text-[10px]">
                      {t("settings.plugins.builtin")}
                    </Badge>
                  )}
                  <Badge variant="outline" className="text-[10px] font-normal">
                    {plugin.kind === "single" ? t("settings.plugins.singleFile") : t("settings.plugins.manifestDir")}
                  </Badge>
                </div>
                <p className="mt-1 truncate text-xs text-muted-foreground">{plugin.description}</p>
              </div>
              <div className="flex items-center gap-2">
                {plugin.settingsSchema && (
                  <Button
                    variant="ghost"
                    size="icon"
                    className="text-muted-foreground"
                    title={t("settings.plugins.settings")}
                    onClick={() => setSettingsFor(plugin)}
                  >
                    <Settings2 size={15} />
                  </Button>
                )}
                <Switch checked={plugin.enabled} onCheckedChange={() => void toggle(plugin)} />
                <Button
                  variant="ghost"
                  size="icon"
                  className="text-muted-foreground hover:text-destructive"
                  title={t("settings.plugins.uninstall")}
                  onClick={() => void uninstall(plugin)}
                >
                  <Trash2 size={15} />
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

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
