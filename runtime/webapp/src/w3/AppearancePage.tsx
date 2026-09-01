// W3 设置中心 · 外观页:两级换肤 UI(规格 §1)。
// 一级 = 主题预设四选(卡片);二级 = 当前主题自带的设置项清单
// (每主题 schema 不同,动态渲染)。改动即时生效(实时预览)并持久化。
// 顶部另设全局偏好(文字大小,§4 裁定:与主题无关)。
import { useState } from "react";
import {
  THEMES,
  THEME_ORDER,
  loadFontPref,
  loadThemeState,
  saveFontPref,
  saveThemeState,
  applyTheme,
  type ThemeDef,
  type ThemeFieldValue,
} from "./themes";
import {
  MonitorIcon,
  ScrollTextIcon,
  StickerIcon,
  DropletsIcon,
} from "lucide-react";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { MonitorIcon, ScrollTextIcon, StickerIcon, DropletsIcon } from "lucide-react";
import { cn } from "@/lib/utils";

const THEME_LOGOS = {
  modern: MonitorIcon,
  classic: ScrollTextIcon,
  cartoon: StickerIcon,
  glass: DropletsIcon,
} as const;

export function AppearancePage() {
  const [state, setState] = useState(loadThemeState);
  const [fontSize, setFontSize] = useState(loadFontPref);

  const update = (next: typeof state) => {
    setState(next);
    saveThemeState(next);
    applyTheme(next); // 实时预览:全站令牌即时切换
  };

  const pickTheme = (id: ThemeDef["id"]) => {
    if (id === state.theme) return;
    update({ ...state, theme: id });
  };

  const setField = (key: string, value: ThemeFieldValue) => {
    const cur = state.settings[state.theme] ?? {};
    update({
      ...state,
      settings: { ...state.settings, [state.theme]: { ...cur, [key]: value } },
    });
  };

  const def = THEMES[state.theme];
  const saved = state.settings[state.theme] ?? {};

  const changeFont = (px: number) => {
    setFontSize(px);
    saveFontPref(px);
    document.documentElement.style.setProperty("--font-size-root", `${px}px`);
  };

  return (
    <div className="flex flex-col gap-5">
      <div>
        <h2 className="text-[15px] font-semibold">外观</h2>
        <p className="text-muted-foreground text-[12.5px]">
          一级选主题,二级调该主题自带的设置项(项集随主题不同);改动即时生效并自动保存。
        </p>
      </div>

      {/* 全局偏好(不属于任何主题) */}
      <section className="bg-card flex items-center gap-4 rounded-xl border p-3">
        <div className="min-w-0 flex-1">
          <div className="text-[13px] font-medium">文字大小</div>
          <div className="text-muted-foreground text-[12px]">
            全局偏好,与主题无关(11–17px)
          </div>
        </div>
        <input
          type="range"
          min={11}
          max={17}
          step={0.5}
          value={fontSize}
          onChange={(e) => changeFont(Number(e.target.value))}
          className="w-40"
          data-slot="font-size-range"
        />
        <span className="w-14 text-right font-mono text-[12px]">
          {fontSize}px
        </span>
      </section>

      {/* 一级:主题预设(用户裁定:一种风格一个小 LOGO,横排点选) */}
      <section className="flex flex-col gap-2">
        <div className="text-[13px] font-medium">主题</div>
        <div className="flex items-start gap-3" data-slot="theme-logos">
          {THEME_ORDER.map((id) => {
            const t = THEMES[id];
            const active = id === state.theme;
            const Logo = THEME_LOGOS[id];
            return (
              <button
                key={id}
                onClick={() => pickTheme(id)}
                className={cn(
                  "flex w-20 flex-col items-center gap-1.5 rounded-xl border p-2.5 transition-colors",
                  active
                    ? "border-[var(--primary)] bg-accent/40 ring-[var(--primary)] ring-1"
                    : "hover:bg-accent/40",
                )}
                data-slot="theme-logo"
                data-theme-id={id}
                title={t.label}
              >
                <span
                  className="flex size-9 items-center justify-center rounded-full text-white"
                  style={{ background: t.swatch.accent }}
                >
                  <Logo className="size-4" />
                </span>
                <span className="text-[11.5px] font-medium">{t.label}</span>
              </button>
            );
          })}
        </div>
      </section>

      {/* 二级:当前主题的设置项(schema 随主题不同) */}
      <section className="flex flex-col gap-2">
        <div className="text-[13px] font-medium">
          {def.label}主题设置项
        </div>
        <div className="flex flex-col gap-2">
          {def.fields.map((f) => {
            const value = saved[f.key] !== undefined ? saved[f.key] : f.default;
            return (
              <div
                key={f.key}
                className="bg-card flex items-center gap-4 rounded-xl border p-3"
                data-slot="theme-field"
                data-key={f.key}
              >
                <div className="min-w-0 flex-1">
                  <div className="text-[13px] font-medium">{f.label}</div>
                  {f.hint ? (
                    <div className="text-muted-foreground text-[11.5px]">
                      {f.hint}
                    </div>
                  ) : null}
                </div>
                <FieldControl
                  field={f}
                  value={value}
                  onChange={(v) => setField(f.key, v)}
                />
              </div>
            );
          })}
        </div>
      </section>
    </div>
  );
}

function FieldControl({
  field: f,
  value,
  onChange,
}: {
  field: ThemeDef["fields"][number];
  value: ThemeFieldValue;
  onChange: (v: ThemeFieldValue) => void;
}) {
  if (f.type === "color") {
    return (
      <span className="flex items-center gap-2">
        <input
          type="color"
          value={String(value)}
          onChange={(e) => onChange(e.target.value)}
          className="h-8 w-12 cursor-pointer rounded border"
          data-slot="field-color"
        />
        <span className="w-20 font-mono text-[11.5px]">{String(value)}</span>
      </span>
    );
  }
  if (f.type === "range") {
    return (
      <span className="flex items-center gap-2">
        <input
          type="range"
          min={f.min}
          max={f.max}
          step={f.step}
          value={Number(value)}
          onChange={(e) => onChange(Number(e.target.value))}
          className="w-40"
          data-slot="field-range"
        />
        <span className="w-16 text-right font-mono text-[11.5px]">
          {Number(value)}
          {f.unit ?? ""}
        </span>
      </span>
    );
  }
  if (f.type === "toggle") {
    return (
      <Switch
        checked={Boolean(value)}
        onCheckedChange={(v) => onChange(v)}
        data-slot="field-toggle"
      />
    );
  }
  if (f.type === "select") {
    return (
      <span className="flex items-center gap-2">
        <select
          value={String(value)}
          onChange={(e) => onChange(e.target.value)}
          className="border-input bg-background h-8 rounded-md border px-2 text-[12.5px] outline-none"
          data-slot="field-select"
        >
          {(f.options ?? []).map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </span>
    );
  }
  // image:内置默认 + 本机自定义(dataURL 存 localStorage;文件选择即改)
  return (
    <span className="flex items-center gap-2">
      <label className="hover:bg-accent/60 cursor-pointer rounded-md border px-2 py-1.5 text-[12px]">
        换图…
        <input
          type="file"
          accept="image/*"
          className="hidden"
          data-slot="field-image"
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (!file) return;
            const reader = new FileReader();
            reader.onload = () => onChange(String(reader.result));
            reader.readAsDataURL(file);
          }}
        />
      </label>
      {value !== f.default ? (
        <button
          className="text-muted-foreground hover:text-foreground text-[11.5px] underline"
          onClick={() => onChange(f.default)}
        >
          用回默认
        </button>
      ) : null}
    </span>
  );
}
