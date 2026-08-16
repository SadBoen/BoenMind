/**
 * 外观设置：界面形态切换（占位）+ 字体档位 + 主题 + 语言。
 *
 * - 界面形态：桌面形态已退役（2026-08-16 用户拍板：全删除，等软件形态稳定再议），
 *   保留切换卡片占位，点"桌面形态"仅提示，无实际效果。
 * - 字体档位：软件界面字号（全局 rem 缩放）。
 * - 通用外观：主题（亮/暗/系统）+ 界面语言。
 */
import { useEffect, useRef, useState } from "react";
import { useTheme } from "next-themes";
import { useTranslation } from "react-i18next";
import { Droplets, Globe, ImagePlus, Laptop, Monitor, Moon, PanelLeft, Sparkles, Sun, Type, Waves } from "lucide-react";
import { useAppStore } from "@/stores/app-store";
import { toast } from "sonner";
import { LANGS, LANG_NAMES, applyLang, type Lang } from "@/i18n";
import { ACCENTS, FONT_SCALES, applyAccent, applyFontScale, fontScale } from "@/lib/appearance";
import { SKINS, type SkinParam } from "@/skins";
import {
  autoGlassParams,
  BACKGROUND_EFFECTS,
  compressImageFile,
  PRESET_WALLPAPERS,
  sampleImage,
  skinById,
  skinParamValue,
  type PresetWallpaper,
  type SkinBackground,
} from "@/lib/skin";
import { renderFluid } from "@/components/skin/FluidWave";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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

/** 壁纸缩略图：gradient 款用 CSS 渐变；fluid 款（蓝色波浪）用 WebGL 渲染一帧 */
function WallpaperThumb({ preset, dark }: { preset: PresetWallpaper; dark: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = ref.current;
    if (!c || preset.kind !== "fluid") return;
    c.width = 160;
    c.height = 80;
    renderFluid(c, dark);
  }, [preset, dark]);
  if (preset.kind === "fluid") {
    return <canvas ref={ref} className="h-12 w-full rounded-lg" aria-hidden />;
  }
  return (
    <span
      aria-hidden
      className="block h-12 w-full rounded-lg"
      style={{ background: dark ? preset.darkCss : preset.css }}
    />
  );
}

export function AppearanceSettings() {
  const { t, i18n } = useTranslation();
  const { theme, setTheme, resolvedTheme } = useTheme();
  const config = useAppStore((s) => s.config);
  const saveConfig = useAppStore((s) => s.saveConfig);
  const viewMode = useAppStore((s) => s.viewMode);
  const settingsTier = useAppStore((s) => s.settingsTier);
  const accent = useAppStore((s) => s.accent);
  const setAccent = useAppStore((s) => s.setAccent);
  const reduceMotion = useAppStore((s) => s.reduceMotion);
  const setReduceMotion = useAppStore((s) => s.setReduceMotion);
  // 皮肤（风格模板切换）
  const skin = useAppStore((s) => s.skin);
  const setSkin = useAppStore((s) => s.setSkin);
  const skinParams = useAppStore((s) => s.skinParams);
  const setSkinParam = useAppStore((s) => s.setSkinParam);
  const skinBackground = useAppStore((s) => s.skinBackground);
  const setSkinBackground = useAppStore((s) => s.setSkinBackground);
  const skinWallpaper = useAppStore((s) => s.skinWallpaper);
  const setSkinWallpaper = useAppStore((s) => s.setSkinWallpaper);
  const skinAuto = useAppStore((s) => s.skinAuto);
  const setSkinAuto = useAppStore((s) => s.setSkinAuto);
  const backgroundEffect = useAppStore((s) => s.backgroundEffect);
  const setBackgroundEffect = useAppStore((s) => s.setBackgroundEffect);
  const expertMode = settingsTier === "expert";

  const [urlDraft, setUrlDraft] = useState("");
  const fileRef = useRef<HTMLInputElement>(null);

  // 挂载时应用持久化的字体档位（重进设置页后回显）
  useEffect(() => {
    applyFontScale(fontScale());
  }, []);

  /** 自动配色：取样背景图 → 写回色调/透明度/模糊（false = 图片不可读，调用方提示） */
  const autoColorize = async (bg: SkinBackground): Promise<boolean> => {
    if (!skinAuto) return true;
    try {
      const sample = await sampleImage(bg.value);
      const { alpha, blur } = autoGlassParams(sample);
      setSkinParam("hue", Math.round(sample.hue));
      setSkinParam("alpha", alpha);
      setSkinParam("blur", blur);
      return true;
    } catch {
      return false;
    }
  };

  const handleFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    try {
      const dataUrl = await compressImageFile(file);
      setSkinBackground({ kind: "data", value: dataUrl });
      const ok = await autoColorize({ kind: "data", value: dataUrl });
      toast.success(t(ok ? "settings.appearance.skin.bgApplied" : "settings.appearance.skin.bgAppliedNoAuto"));
    } catch (err) {
      toast.error(t("settings.appearance.skin.fileFailed", { error: String(err) }));
    }
  };

  const applyUrl = async () => {
    const url = urlDraft.trim();
    if (!url) return;
    const bg: SkinBackground = { kind: "url", value: url };
    setSkinBackground(bg);
    setUrlDraft("");
    const ok = await autoColorize(bg);
    toast.success(t(ok ? "settings.appearance.skin.bgApplied" : "settings.appearance.skin.bgAppliedNoAuto"));
  };

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

      {/* 皮肤（风格模板切换，2026-08-16）：不改布局，只换材质风格；玻璃皮肤可调
          色调/透明度/模糊 + 背景图（本地文件或 URL），支持按图自动配色 */}
      <div className="border-t pt-5">
        <h3 className="text-sm font-semibold">{t("settings.appearance.skin.title")}</h3>
        <p className="text-xs text-muted-foreground">{t("settings.appearance.skin.desc")}</p>
        <div className="mt-3 grid grid-cols-2 gap-3">
          {SKINS.map((s) => (
            <button
              key={s.id}
              type="button"
              onClick={() => {
                setSkin(s.id);
                toast.success(t("settings.appearance.saved"));
              }}
              className={cn(
                "flex flex-col items-center gap-2 rounded-xl border-2 p-4 transition-colors",
                skin === s.id
                  ? "border-primary bg-primary/5"
                  : "border-border hover:border-muted-foreground/40",
              )}
            >
              <span
                aria-hidden
                className={cn(
                  "flex h-12 w-full items-center justify-center rounded-lg border border-border",
                  s.id === "glass" && "bg-gradient-to-br from-primary/35 via-primary/15 to-transparent",
                )}
              >
                {s.id === "glass" ? (
                  <Droplets size={18} className={skin === s.id ? "text-primary" : "text-muted-foreground"} />
                ) : (
                  <PanelLeft size={18} className={skin === s.id ? "text-primary" : "text-muted-foreground"} />
                )}
              </span>
              <span className="text-sm font-medium">{t(s.nameKey)}</span>
              <span className="text-center text-[11px] leading-snug text-muted-foreground">{t(s.descKey)}</span>
            </button>
          ))}
        </div>

        {/* 玻璃皮肤参数面板 */}
        {skin === "glass" && (
          <div className="mt-4 space-y-4 rounded-xl border p-4">
            {/* 预设壁纸（Aqua 观感流体渐变四款，明暗自适应；选中自动套推荐色调） */}
            <div>
              <p className="text-xs font-medium text-muted-foreground">{t("settings.appearance.skin.wallpaperTitle")}</p>
              <p className="text-xs text-muted-foreground">{t("settings.appearance.skin.wallpaperDesc")}</p>
              <div className="mt-2 grid grid-cols-5 gap-2">
                {PRESET_WALLPAPERS.map((w) => (
                  <button
                    key={w.id}
                    type="button"
                    title={t(w.nameKey)}
                    onClick={() => {
                      setSkinWallpaper(w.id);
                      setSkinParam("hue", w.hue);
                      toast.success(t("settings.appearance.saved"));
                    }}
                    className={cn(
                      "overflow-hidden rounded-lg border-2 transition-colors",
                      skinWallpaper === w.id
                        ? "border-primary"
                        : "border-border hover:border-muted-foreground/40",
                    )}
                  >
                    <WallpaperThumb preset={w} dark={resolvedTheme === "dark"} />
                  </button>
                ))}
              </div>
            </div>

            {/* 背景特效（独立于皮肤/壁纸的动画层开关，2026-08-16） */}
            <div>
              <p className="text-xs font-medium text-muted-foreground">{t("settings.appearance.skin.effectTitle")}</p>
              <p className="text-xs text-muted-foreground">{t("settings.appearance.skin.effectDesc")}</p>
              <div className="mt-2 grid grid-cols-2 gap-2">
                {BACKGROUND_EFFECTS.map((e) => (
                  <button
                    key={e.id}
                    type="button"
                    onClick={() => {
                      setBackgroundEffect(e.id);
                      toast.success(t("settings.appearance.saved"));
                    }}
                    className={cn(
                      "flex items-center justify-center gap-1.5 rounded-lg border-2 px-3 py-2 text-xs font-medium transition-colors",
                      backgroundEffect === e.id
                        ? "border-primary bg-primary/5 text-primary"
                        : "border-border text-muted-foreground hover:border-muted-foreground/40",
                    )}
                  >
                    {e.id === "wave" && <Waves size={13} />}
                    {t(e.nameKey)}
                  </button>
                ))}
              </div>
            </div>

            {/* 自定义背景：本地文件（压缩存储）/ URL 直链 */}
            <div>
              <p className="text-xs font-medium text-muted-foreground">{t("settings.appearance.skin.customTitle")}</p>
              <p className="text-xs text-muted-foreground">{t("settings.appearance.skin.backgroundDesc")}</p>
              <div className="mt-2 flex flex-wrap items-center gap-2">
                <Button variant="outline" size="sm" onClick={() => fileRef.current?.click()}>
                  <ImagePlus size={14} />
                  {t("settings.appearance.skin.selectFile")}
                </Button>
                <Input
                  value={urlDraft}
                  onChange={(e) => setUrlDraft(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && void applyUrl()}
                  placeholder={t("settings.appearance.skin.urlPlaceholder")}
                  className="h-8 w-56"
                />
                <Button variant="outline" size="sm" onClick={() => void applyUrl()} disabled={!urlDraft.trim()}>
                  {t("settings.appearance.skin.apply")}
                </Button>
                {(skinBackground || skinWallpaper) && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      setSkinBackground(null);
                      setSkinWallpaper(null);
                    }}
                  >
                    {t("settings.appearance.skin.remove")}
                  </Button>
                )}
              </div>
              <input ref={fileRef} type="file" accept="image/*" hidden onChange={handleFile} />
              {skinBackground && (
                <img
                  src={skinBackground.value}
                  alt=""
                  className="mt-2 h-16 w-28 rounded-lg border border-border object-cover"
                />
              )}
            </div>

            {/* 自动配色开关 */}
            <div className="flex items-center justify-between gap-3 rounded-lg border p-3">
              <div className="min-w-0">
                <p className="text-sm font-medium">{t("settings.appearance.skin.autoTitle")}</p>
                <p className="text-xs text-muted-foreground">{t("settings.appearance.skin.autoDesc")}</p>
              </div>
              <Switch checked={skinAuto} onCheckedChange={setSkinAuto} />
            </div>

            {/* 参数滑杆（注册表声明驱动） */}
            {skinById(skin).params.map((p: SkinParam) => (
              <div key={p.key}>
                <div className="flex items-center justify-between text-xs">
                  <span className="font-medium text-muted-foreground">{t(p.labelKey)}</span>
                  <span className="tabular-nums text-foreground">
                    {skinParamValue(skin, skinParams, p.key)}
                    {p.format === "percent" ? "%" : p.format === "px" ? "px" : "°"}
                  </span>
                </div>
                <input
                  type="range"
                  min={p.min}
                  max={p.max}
                  step={p.step}
                  value={skinParamValue(skin, skinParams, p.key)}
                  onChange={(e) => setSkinParam(p.key, Number(e.target.value))}
                  className="skin-range mt-1.5 w-full"
                />
              </div>
            ))}
          </div>
        )}
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
