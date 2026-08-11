/**
 * 外观设置：主题（亮色 / 暗色 / 跟随系统）+ 界面语言（中文 / English / 日本語 / 한국어）。
 * 两者均即时生效并持久化到后端 config（桌面端与网页端一致）。
 */
import { useTheme } from "next-themes";
import { useTranslation } from "react-i18next";
import { Globe, Laptop, Moon, Sun } from "lucide-react";
import { useAppStore } from "@/stores/app-store";
import { toast } from "sonner";
import { LANGS, LANG_NAMES, applyLang, type Lang } from "@/i18n";

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

      <div className="grid grid-cols-3 gap-3">
        {THEMES.map((th) => (
          <button
            key={th.key}
            type="button"
            onClick={() => void applyTheme(th.key)}
            className={`flex flex-col items-center gap-2 rounded-xl border-2 p-4 transition-colors ${
              theme === th.key
                ? "border-primary bg-primary/5"
                : "border-border hover:border-muted-foreground/40"
            }`}
          >
            {th.icon}
            <span className="text-sm font-medium">{t(th.labelKey)}</span>
          </button>
        ))}
      </div>

      <p className="text-xs text-muted-foreground">{t("settings.appearance.hint")}</p>

      {/* 界面语言 */}
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
              className={`flex flex-col items-center gap-2 rounded-xl border-2 p-4 transition-colors ${
                i18n.language === lang
                  ? "border-primary bg-primary/5"
                  : "border-border hover:border-muted-foreground/40"
              }`}
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
