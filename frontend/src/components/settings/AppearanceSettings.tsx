/**
 * 外观设置：界面形态切换（软件形态 / 桌面形态）+ 各形态专属设置 + 通用外观。
 *
 * - 界面形态：软件形态与桌面形态的切换开关（即双 DE 切换，与导航条/菜单栏
 *   入口同一 store）；**只显示当前形态的专属设置区**（软件形态：字体大小；
 *   桌面形态：桌面模板壁纸）。
 * - 通用外观：主题（亮/暗/系统）+ 界面语言（两种形态共用）。
 */
import { useEffect } from "react";
import { useTheme } from "next-themes";
import { useTranslation } from "react-i18next";
import { Globe, Laptop, Monitor, Moon, PanelLeft, Sun, Type } from "lucide-react";
import { useAppStore } from "@/stores/app-store";
import { toast } from "sonner";
import { LANGS, LANG_NAMES, applyLang, type Lang } from "@/i18n";
import { FONT_SCALES, WALLPAPERS, applyFontScale, fontScale } from "@/lib/appearance";
import { cn } from "@/lib/utils";

const THEMES = [
  { key: "light", labelKey: "settings.appearance.light", icon: <Sun size={16} /> },
  { key: "dark", labelKey: "settings.appearance.dark", icon: <Moon size={16} /> },
  { key: "system", labelKey: "settings.appearance.system", icon: <Laptop size={16} /> },
] as const;

export function AppearanceSettings() {
  const { t, i18n } = useTranslation();
  const { theme, setTheme } = useTheme();
  const config = useAppStore((s) => s.config);
  const saveConfig = useAppStore((s) => s.saveConfig);
  const viewMode = useAppStore((s) => s.viewMode);
  const setViewMode = useAppStore((s) => s.setViewMode);
  const wallpaper = useAppStore((s) => s.wallpaper);
  const setWallpaper = useAppStore((s) => s.setWallpaper);

  // 挂载时应用持久化的字体档位（切换形态/重进设置页后回显）
  useEffect(() => {
    applyFontScale(fontScale());
  }, []);

  const applyTheme = async (key: string) => {
    setTheme(key);
    if (config) {
      try {
        await saveConfig({ ...config, theme: key });
        toast.success(t("settings.appearance.saved"));
      } catch (err) {
        toast.error(t("settings.appearance.saveFailed", { error: String(err) }));
      }
    }
  };

  const handleLangChange = async (key: Lang) => {
    // 等待语言切换完成，toast 用全局 i18n.t（hook 的 t 绑定在 render 时，异步回调里仍是旧语言）
    await applyLang(key);
    if (config) {
      try {
        await saveConfig({ ...config, lang: key });
        toast.success(i18n.t("settings.appearance.langSaved"));
      } catch (err) {
        toast.error(i18n.t("settings.appearance.saveFailed", { error: String(err) }));
      }
    }
  };

  return (
    <section className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">{t("settings.appearance.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("settings.appearance.desc")}</p>
      </div>

      {/* 界面形态：软件形态 / 桌面形态切换（与导航条/菜单栏入口同一状态） */}
      <div>
        <h3 className="text-sm font-semibold">{t("settings.appearance.form.title")}</h3>
        <p className="text-xs text-muted-foreground">{t("settings.appearance.form.desc")}</p>
        <div className="mt-3 grid grid-cols-2 gap-3">
          <button
            type="button"
            onClick={() => setViewMode("classic")}
            className={cn(
              "flex flex-col items-center gap-2 rounded-xl border-2 p-4 transition-colors",
              viewMode === "classic"
                ? "border-primary bg-primary/5"
                : "border-border hover:border-muted-foreground/40",
            )}
          >
            <PanelLeft size={18} className={viewMode === "classic" ? "text-primary" : ""} />
            <span className="text-sm font-medium">{t("settings.appearance.form.classic")}</span>
          </button>
          <button
            type="button"
            onClick={() => setViewMode("desktop")}
            className={cn(
              "flex flex-col items-center gap-2 rounded-xl border-2 p-4 transition-colors",
              viewMode === "desktop"
                ? "border-primary bg-primary/5"
                : "border-border hover:border-muted-foreground/40",
            )}
          >
            <Monitor size={18} className={viewMode === "desktop" ? "text-primary" : ""} />
            <span className="text-sm font-medium">{t("settings.appearance.form.desktop")}</span>
          </button>
        </div>
      </div>

      {/* 形态专属设置：只显示当前形态的对应设置（软件形态=字体大小；桌面形态=壁纸模板） */}
      {viewMode === "classic" ? (
        <div>
          <h3 className="flex items-center gap-1.5 text-sm font-semibold">
            <Type size={14} />
            {t("settings.appearance.font.title")}
          </h3>
          <p className="text-xs text-muted-foreground">{t("settings.appearance.font.desc")}</p>
          <div className="mt-3 grid grid-cols-3 gap-3">
            {FONT_SCALES.map((f) => (
              <button
                key={f.key}
                type="button"
                onClick={() => {
                  applyFontScale(f.key);
                  toast.success(t("settings.appearance.saved"));
                }}
                className={cn(
                  "flex flex-col items-center gap-1 rounded-xl border-2 p-3 transition-colors",
                  fontScale() === f.key
                    ? "border-primary bg-primary/5"
                    : "border-border hover:border-muted-foreground/40",
                )}
                style={{ fontSize: f.px }}
              >
                <span className="text-sm font-medium">{t(`settings.appearance.font.${f.key}`)}</span>
              </button>
            ))}
          </div>
        </div>
      ) : (
        <div>
          <h3 className="text-sm font-semibold">{t("settings.appearance.wallpaper.title")}</h3>
          <p className="text-xs text-muted-foreground">{t("settings.appearance.wallpaper.desc")}</p>
          <div className="mt-3 grid grid-cols-2 gap-3">
            {WALLPAPERS.map((w) => (
              <button
                key={w.key}
                type="button"
                onClick={() => {
                  setWallpaper(w.key);
                  toast.success(t("settings.appearance.saved"));
                }}
                className={cn(
                  "flex h-16 flex-col items-center justify-center gap-1 rounded-xl border-2 transition-colors",
                  wallpaper === w.key
                    ? "border-primary bg-primary/5"
                    : "border-border hover:border-muted-foreground/40",
                )}
              >
                <span
                  aria-hidden
                  className={cn(
                    "h-6 w-10 rounded-md border border-black/10",
                    w.key === "starry" ? "desktop-wallpaper-preview-starry" : "desktop-wallpaper-preview-aurora",
                  )}
                />
                <span className="text-xs font-medium">{t(w.labelKey)}</span>
              </button>
            ))}
          </div>
        </div>
      )}

      {/* 通用外观：主题（模板） */}
      <div className="border-t pt-5">
        <h3 className="text-sm font-semibold">{t("settings.appearance.themeTitle")}</h3>
        <p className="text-xs text-muted-foreground">{t("settings.appearance.themeDesc")}</p>
        <div className="mt-3 grid grid-cols-3 gap-3">
          {THEMES.map((th) => (
            <button
              key={th.key}
              type="button"
              onClick={() => void applyTheme(th.key)}
              className={cn(
                "flex flex-col items-center gap-2 rounded-xl border-2 p-4 transition-colors",
                theme === th.key
                  ? "border-primary bg-primary/5"
                  : "border-border hover:border-muted-foreground/40",
              )}
            >
              {th.icon}
              <span className="text-sm font-medium">{t(th.labelKey)}</span>
            </button>
          ))}
        </div>
        <p className="mt-3 text-xs text-muted-foreground">{t("settings.appearance.hint")}</p>
      </div>

      {/* 通用外观：界面语言 */}
      <div className="border-t pt-5">
        <div>
          <h2 className="text-lg font-semibold">{t("settings.appearance.langTitle")}</h2>
          <p className="text-sm text-muted-foreground">{t("settings.appearance.langDesc")}</p>
        </div>

        <div className="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4">
          {LANGS.map((lang) => (
            <button
              key={lang}
              type="button"
              onClick={() => void handleLangChange(lang)}
              className={cn(
                "flex flex-col items-center gap-2 rounded-xl border-2 p-4 transition-colors",
                i18n.language === lang
                  ? "border-primary bg-primary/5"
                  : "border-border hover:border-muted-foreground/40",
              )}
            >
              <Globe size={16} className={i18n.language === lang ? "text-primary" : ""} />
              <span className="text-sm font-medium">{LANG_NAMES[lang]}</span>
            </button>
          ))}
        </div>

        <p className="mt-3 text-xs text-muted-foreground">{t("settings.appearance.langHint")}</p>
      </div>
    </section>
  );
}
