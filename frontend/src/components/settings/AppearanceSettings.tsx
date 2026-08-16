/**
 * 外观设置：界面形态切换（占位）+ 字体档位 + 主题 + 语言。
 *
 * - 界面形态：桌面形态已退役（2026-08-16 用户拍板：全删除，等软件形态稳定再议），
 *   保留切换卡片占位，点"桌面形态"仅提示，无实际效果。
 * - 字体档位：软件界面字号（全局 rem 缩放）。
 * - 通用外观：主题（亮/暗/系统）+ 界面语言。
 */
import { useEffect } from "react";
import { useTheme } from "next-themes";
import { useTranslation } from "react-i18next";
import { Globe, Laptop, Monitor, Moon, PanelLeft, Sparkles, Sun, Type } from "lucide-react";
import { useAppStore } from "@/stores/app-store";
import { toast } from "sonner";
import { LANGS, LANG_NAMES, applyLang, type Lang } from "@/i18n";
import { ACCENTS, FONT_SCALES, applyAccent, applyFontScale, fontScale } from "@/lib/appearance";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

const THEMES = [
  { key: "light", labelKey: "settings.appearance.light", icon: <Sun size={16} /> },
  { key: "dark", labelKey: "settings.appearance.dark", icon: <Moon size={16} /> },
  { key: "system", labelKey: "settings.appearance.system", icon: <Laptop size={16} /> },
] as const;

/** 强调色预览色（与 index.css 的 data-accent 覆盖一致；default = 当前前景色） */
const ACCENT_SWATCH: Record<string, string> = {
  default: "var(--foreground)",
  violet: "oklch(0.5 0.2 290)",
  blue: "oklch(0.52 0.19 255)",
  green: "oklch(0.52 0.17 155)",
  orange: "oklch(0.62 0.18 45)",
  pink: "oklch(0.58 0.2 350)",
};

export function AppearanceSettings() {
  const { t, i18n } = useTranslation();
  const { theme, setTheme } = useTheme();
  const config = useAppStore((s) => s.config);
  const saveConfig = useAppStore((s) => s.saveConfig);
  const viewMode = useAppStore((s) => s.viewMode);
  const settingsTier = useAppStore((s) => s.settingsTier);
  const accent = useAppStore((s) => s.accent);
  const setAccent = useAppStore((s) => s.setAccent);
  const reduceMotion = useAppStore((s) => s.reduceMotion);
  const setReduceMotion = useAppStore((s) => s.setReduceMotion);
  const expertMode = settingsTier === "expert";

  // 挂载时应用持久化的字体档位（重进设置页后回显）
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

      {/* 界面形态：桌面形态已退役，切换开关仅占位（点桌面仅提示） */}
      <div>
        <h3 className="text-sm font-semibold">{t("settings.appearance.form.title")}</h3>
        <p className="text-xs text-muted-foreground">{t("settings.appearance.form.desc")}</p>
        <div className="mt-3 grid grid-cols-2 gap-3">
          <button
            type="button"
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
            onClick={() => toast.info(t("settings.appearance.form.desktopRemoved"))}
            className={cn(
              "flex flex-col items-center gap-2 rounded-xl border-2 p-4 transition-colors",
              "border-border hover:border-muted-foreground/40",
            )}
          >
            <Monitor size={18} />
            <span className="text-sm font-medium">{t("settings.appearance.form.desktop")}</span>
          </button>
        </div>
      </div>

      {/* 字体档位（软件界面全局字号） */}
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

      {/* 高级外观（资深模式可见）：强调色 + 减少动画 */}
      {expertMode && (
        <div className="rounded-xl border p-4">
          <h3 className="flex items-center gap-1.5 text-sm font-semibold">
            <Sparkles size={14} className="text-muted-foreground" />
            {t("settings.appearance.advancedTitle")}
          </h3>
          <p className="text-xs text-muted-foreground">{t("settings.appearance.advancedDesc")}</p>

          <div className="mt-3">
            <p className="text-xs font-medium text-muted-foreground">{t("settings.appearance.accentTitle")}</p>
            <div className="mt-2 grid grid-cols-3 gap-2 sm:grid-cols-6">
              {ACCENTS.map((a) => (
                <button
                  key={a.key}
                  type="button"
                  onClick={() => {
                    applyAccent(a.key);
                    setAccent(a.key);
                    toast.success(t("settings.appearance.saved"));
                  }}
                  className={cn(
                    "flex flex-col items-center gap-1.5 rounded-xl border-2 p-2 transition-colors",
                    accent === a.key ? "border-primary bg-primary/5" : "border-border hover:border-muted-foreground/40",
                  )}
                >
                  <span
                    aria-hidden
                    className="h-6 w-9 rounded-md border border-black/10"
                    style={{ background: ACCENT_SWATCH[a.key] }}
                  />
                  <span className="text-[10px] font-medium">{t(a.labelKey)}</span>
                </button>
              ))}
            </div>
          </div>

          <div className="mt-4 flex items-center justify-between gap-3 rounded-lg border p-3">
            <div className="min-w-0">
              <p className="text-sm font-medium">{t("settings.appearance.reduceMotionTitle")}</p>
              <p className="text-xs text-muted-foreground">{t("settings.appearance.reduceMotionDesc")}</p>
            </div>
            <Switch
              checked={reduceMotion}
              onCheckedChange={(checked) => setReduceMotion(checked)}
            />
          </div>
        </div>
      )}

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
