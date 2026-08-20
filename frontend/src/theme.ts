// 主题系统：风格档（preset）注册表 + CSS 变量桥接 + 背景注册表。
//
// 设计（Grok 审查后定稿）：
// - "风格档"是唯一维度（明暗维度已按用户要求删除，保持深色基调）。
//   每档输出两份：antd ThemeConfig（给 ConfigProvider）+ 纯 CSS 变量 map（写 --bm-*/--dv-*）。
// - 背景是独立维度：style（默认/渐变/图片）与风格档正交，未来动态背景在此注册。
// - antd 样式经 ConfigProvider prefixCls="bm" 作用域隔离，不打 dockview。
// - z-index 契约（勿用魔法数）：dockview drop < 悬浮标题栏 < antd Modal/Dropdown。
// - 防闪：index.html 内联 bm-pre-theme（背景精简版），此处是完整版，启动即同步。

import type { ThemeConfig } from "antd";
import { theme as antdTheme } from "antd";
import { useEffect } from "react";

// ---- 类型 ----
export type PresetId = "ant" | "cartoon" | "glass";
export type BackgroundId = "default" | "gradient" | "image";
export type BackgroundValue = { type: "none" | "gradient" | "image"; value?: string };

export interface Preset {
  id: PresetId;
  label: string;
  desc: string;
  /** antd 主题配置 */
  antd: ThemeConfig;
  /** CSS 变量（--bm-* + --dv-* 桥接），写进 <style id="bm-theme"> */
  cssVars: Record<string, string>;
  /** 附加样式类（如玻璃的 backdrop-filter） */
  className?: string;
}

export interface BackgroundDef {
  id: BackgroundId;
  label: string;
  desc: string;
  /** 应用到 body 的背景 CSS（含前缀；default 为 null 表示用主题底色） */
  css?: string | null;
  /** 是否允许上传本地图片（image 档） */
  uploadable?: boolean;
}

// ---- 存储 key（与 index.html 防闪脚本同源）----
const PRESET_KEY = "bm_preset";
const BACKGROUND_KEY = "bm_background";
const ACCENT_KEY = "bm_accent";
const FONT_KEY = "bm_fontsize";
const GLASS_OPACITY_KEY = "bm_glass_opacity";

/** 玻璃档面板透明度（0.2–0.95，默认 0.68）。仅玻璃档生效。 */
export function getGlassOpacity(): number {
  const v = Number(localStorage.getItem(GLASS_OPACITY_KEY));
  if (Number.isFinite(v) && v >= 0.2 && v <= 0.95) return Math.round(v * 100) / 100;
  return 0.68;
}
export function setGlassOpacity(v: number) {
  localStorage.setItem(GLASS_OPACITY_KEY, String(Math.round(v * 100) / 100));
}

export function getPresetId(): PresetId {
  const v = localStorage.getItem(PRESET_KEY);
  if (v === "glass" || v === "cartoon") return v as PresetId;
  if (v === "mui") return "cartoon"; // 兼容旧 key
  return "ant";
}
export function setPresetId(id: PresetId) {
  localStorage.setItem(PRESET_KEY, id);
}
export function getBackground(): BackgroundValue {
  try {
    const v = JSON.parse(localStorage.getItem(BACKGROUND_KEY) || "null");
    if (v && typeof v === "object") return v as BackgroundValue;
  } catch { /* ignore */ }
  return { type: "none" };
}
export function setBackground(v: BackgroundValue) {
  localStorage.setItem(BACKGROUND_KEY, JSON.stringify(v));
}
export function getAccent(): string | null {
  // 用户显式自定义的强调色；无则 null → 主题档用自己的 accent（不全局覆盖）。
  const v = localStorage.getItem(ACCENT_KEY);
  return v && /^#[0-9a-fA-F]{6}$/.test(v.trim()) ? v.trim() : null;
}
export function setAccent(v: string) {
  localStorage.setItem(ACCENT_KEY, v);
}
export function getFontSize(): number {
  const v = Number(localStorage.getItem(FONT_KEY));
  return v >= 12 && v <= 18 ? v : 14;
}
export function setFontSize(v: number) {
  localStorage.setItem(FONT_KEY, String(v));
}

// ---- 4 个风格档 ----
// 每档的颜色都围绕同一套暗色系设计：bg 基底 / bg-2 面板 / bg-3 悬停 / border / fg 文字 / accent 强调。
export const PRESETS: Record<PresetId, Preset> = {
  ant: {
    id: "ant",
    label: "黑白",
    desc: "Graphite Editorial：黑白灰分层 + 克制蓝焦点（桌面工具）",
    antd: {
      algorithm: antdTheme.defaultAlgorithm,
      token: {
        colorPrimary: "#2563EB",
        colorError: "#DC2626",
        colorBgBase: "#F5F6FA",
        colorBgContainer: "#FFFFFF",
        colorBgElevated: "#FFFFFF",
        colorBorder: "#E2E5EB",
        colorTextBase: "#1A1D23",
        colorTextSecondary: "#5C6370",
        borderRadius: 6,
        fontSize: 14,
      },
    },
    cssVars: {
      "--bm-bg": "#F5F6FA",
      "--bm-bg-2": "#FFFFFF",
      "--bm-bg-3": "#ECEEF3",
      "--bm-bg-glass": "rgba(245, 246, 250, 0.92)",
      "--bm-panel-mid": "#F5F6FA",
      "--bm-border": "#E2E5EB",
      "--bm-border-strong": "#C5CAD6",
      "--bm-border-subtle": "#EEEFF3",
      "--bm-fg": "#1A1D23",
      "--bm-fg-dim": "#5C6370",
      "--bm-accent": "#2563EB",
      "--bm-accent-hover": "#1D4ED8",
      "--bm-accent-2": "#1E40AF",
      "--bm-accent-soft": "rgba(37, 99, 235, 0.08)",
      "--bm-danger": "#DC2626",
      "--bm-radius": "6px",
    },
  },
  cartoon: {
    id: "cartoon",
    label: "卡通",
    desc: "Kraft Journal：暖米牛皮纸 + sage 墨绿 + 大圆角（手账贴纸）",
    antd: {
      algorithm: antdTheme.defaultAlgorithm, // 浅色
      token: {
        colorPrimary: "#3E6B5E",
        colorPrimaryHover: "#2F5448",
        colorError: "#C45C4A",
        colorSuccess: "#5A8A7A",
        colorBgBase: "#E8DDC9",
        colorBgContainer: "#F7F1E6",
        colorBgElevated: "#F7F1E6",
        colorBorder: "#D4C4A8",
        colorTextBase: "#2C2416",
        colorTextSecondary: "#6B5D4D",
        borderRadius: 16,
        borderRadiusLG: 24,
        borderRadiusSM: 12,
        controlHeight: 40,
        fontSize: 14,
      },
      components: {
        Tag: { borderRadiusSM: 9999, defaultBg: "#F7F1E6" },
        Button: { borderRadius: 18, controlHeight: 38, primaryColor: "#F7F1E6" },
        Input: { borderRadius: 14 },
        Segmented: { itemSelectedBg: "#3E6B5E", itemSelectedColor: "#F7F1E6", borderRadius: 9999 },
      },
    },
    cssVars: {
      "--bm-bg": "#E8DDC9",
      "--bm-bg-2": "#F7F1E6",
      "--bm-bg-3": "#E4D8C0",
      "--bm-bg-glass": "rgba(247, 241, 230, 0.88)",
      "--bm-panel-mid": "rgba(62, 107, 94, 0.06)",
      "--bm-border": "#D4C4A8",
      "--bm-border-strong": "#C4B090",
      "--bm-border-subtle": "#E5D9C4",
      "--bm-fg": "#2C2416",
      "--bm-fg-dim": "#6B5D4D",
      "--bm-accent": "#3E6B5E",
      "--bm-accent-hover": "#2F5448",
      "--bm-accent-2": "#3E6B5E",
      "--bm-accent-soft": "rgba(62, 107, 94, 0.12)",
      "--bm-danger": "#C45C4A",
      "--bm-radius": "20px",
    },
    className: "preset-cartoon",
  },
  glass: {
    id: "glass",
    label: "玻璃",
    desc: "透明分层：单一深色背景 + 黑色不透明度分层 + 白边框（无面板色）",
    antd: {
      algorithm: antdTheme.darkAlgorithm,
      token: {
        colorPrimary: "#9AABB7",
        colorError: "#FB7185",
        colorBgBase: "#090A0C",
        colorBgContainer: "rgba(0, 0, 0, 0.42)",
        colorBgElevated: "rgba(0, 0, 0, 0.90)",
        colorBorder: "rgba(255, 255, 255, 0.09)",
        colorTextBase: "rgba(255, 255, 255, 0.92)",
        colorTextSecondary: "rgba(255, 255, 255, 0.62)",
        colorTextLightSolid: "rgba(255,255,255,0.95)",
        borderRadius: 10,
        fontSize: 14,
      },
    },
    cssVars: {
      // 唯一背景（深炭黑 + 中性高光渐变在 styles.css preset-glass body）
      "--bm-bg": "#090A0C",
      // 结构面：全黑纱，α 分层（相对滑块 --g 的偏移；默认 --g=0.42）
      "--bm-bg-2": "rgba(0, 0, 0, 0.42)",
      "--bm-bg-3": "rgba(0, 0, 0, 0.60)",
      "--bm-bg-glass": "rgba(0, 0, 0, 0.42)",
      "--bm-panel-mid": "rgba(0, 0, 0, 0.17)",
      // 内容面全透黑纱底（气泡/菜单等由样式单独给）
      "--bm-bg-glass-light": "rgba(255, 255, 255, 0.045)",
      // 边框全白 alpha 三档
      "--bm-border": "rgba(255, 255, 255, 0.09)",
      "--bm-border-strong": "rgba(255, 255, 255, 0.15)",
      "--bm-border-subtle": "rgba(255, 255, 255, 0.055)",
      // 文字全白 alpha 层级
      "--bm-fg": "rgba(255, 255, 255, 0.92)",
      "--bm-fg-dim": "rgba(255, 255, 255, 0.62)",
      // accent 极克制：雾蓝灰，仅线/焦点环/主按钮
      "--bm-accent": "#9AABB7",
      "--bm-accent-hover": "rgba(154, 171, 191, 0.60)",
      "--bm-accent-2": "rgba(154, 171, 191, 0.45)",
      "--bm-accent-soft": "rgba(154, 171, 191, 0.20)",
      "--bm-danger": "#FB7185",
      "--bm-radius": "10px",
    },
    className: "preset-glass",
  },
};

// ---- 背景注册表（独立维度；未来动态背景在此注册）----
export const BACKGROUNDS: BackgroundDef[] = [
  { id: "default", label: "默认", desc: "跟随风格档底色", css: null },
  {
    id: "gradient",
    label: "渐变",
    desc: "深色渐变背景",
    css: "linear-gradient(160deg, #0f1420 0%, #1a2340 55%, #0f1420 100%)",
  },
  {
    id: "image",
    label: "图片",
    desc: "本地图片或图片 URL（动态背景预留位）",
    css: null,
    uploadable: true,
  },
];

// ---- 应用 CSS 变量到 :root（桥接 antd token ↔ --bm-* / --dv-*）----
export function applyPresetCss(preset: Preset, accent: string | null) {
  const vars: Record<string, string> = { ...preset.cssVars };
  // 玻璃档：滑块只绑结构层黑纱 α（--bm-bg-glass 的面板），内容层（气泡/菜单/代码）
  // 锁死不随滑块——保证极端透明度下文字仍可读。默认 0.42（Grok α 分层）。
  if (preset.id === "glass") {
    const g = getGlassOpacity();
    vars["--bm-bg-glass"] = `rgba(0, 0, 0, ${Math.round(g * 100) / 100})`;
    // 结构面相对滑块偏移（nav/sidebar/status 比 panel 实一档；聊天画布最透）
    vars["--bm-bg-2"] = `rgba(0, 0, 0, ${Math.min(g + 0.08, 0.92)})`;
    vars["--bm-bg-3"] = `rgba(0, 0, 0, ${Math.min(g + 0.18, 0.92)})`;
    vars["--bm-panel-mid"] = `rgba(0, 0, 0, ${Math.round(g * 0.4 * 100) / 100})`;
    // blur 随透明度微减（越实越不需要霜）
    vars["--bm-blur-shell"] = `${Math.round(28 - g * 10)}px`;
  }
  // dockview 桥接变量
  vars["--dv-background-color"] = vars["--bm-bg-glass"];
  vars["--dv-group-view-background-color"] = vars["--bm-bg-glass"];
  vars["--dv-tabs-and-actions-container-background-color"] = vars["--bm-bg-glass"];
  vars["--dv-tab-active-background-color"] = preset.cssVars["--bm-bg-3"];
  vars["--dv-tab-inactive-background-color"] = vars["--bm-bg-glass"];
  vars["--dv-tab-active-color"] = preset.cssVars["--bm-fg"];
  vars["--dv-tab-inactive-color"] = preset.cssVars["--bm-fg-dim"];
  vars["--dv-tabs-container-background-color"] = vars["--bm-bg-glass"];
  vars["--dv-active-sash-color"] = preset.cssVars["--bm-accent"];
  vars["--dv-separator-border"] = preset.cssVars["--bm-border-strong"];
  // 强调色：仅当用户显式自定义（localStorage bm_accent）时覆盖；否则各档用自己
  // 的 accent（卡通墨绿 / 玻璃紫），不再拿全局默认蓝顶掉档内配色。
  if (accent && /^#[0-9a-fA-F]{6}$/.test(accent)) {
    vars["--bm-accent"] = accent;
    vars["--bm-accent-2"] = accent + "cc";
  }
  let css = "";
  for (const [k, v] of Object.entries(vars)) css += `${k}:${v};`;
  let style = document.getElementById("bm-theme") as HTMLStyleElement | null;
  if (!style) {
    style = document.createElement("style");
    style.id = "bm-theme";
    document.head.appendChild(style);
  }
  style.textContent = `:root{${css}}`;
  // 风格档 className（玻璃的 backdrop-filter 等）
  const root = document.documentElement;
  root.dataset.preset = preset.id;
  for (const p of Object.values(PRESETS)) {
    if (p.className) root.classList.remove(p.className);
  }
  if (preset.className) root.classList.add(preset.className);
}

// ---- 应用背景到 body（none=跟随主题底色 / gradient=渐变 / image=图片 URL）----
export function applyBackground(bg: BackgroundValue) {
  const body = document.body;
  const root = document.documentElement;
  if (bg.type === "image" && bg.value) {
    body.style.background = `url("${bg.value}") center/cover fixed no-repeat`;
  } else if (bg.type === "gradient") {
    const grad = BACKGROUNDS.find((b) => b.id === "gradient");
    body.style.background = grad?.css ?? "";
    body.style.backgroundSize = "cover";
  } else {
    body.style.background = ""; // 回落 body{background:var(--bm-bg)}
  }
  root.style.background = ""; // 防闪脚本设置的 background 由这里接管
}

// ---- 应用字号 ----
export function applyFontSize(size: number) {
  document.documentElement.style.fontSize = size + "px";
}

// ---- React 侧同步钩子：挂载时把风格档/背景/字号落到 DOM ----
export function useThemeSync() {
  useEffect(() => {
    const preset = PRESETS[getPresetId()];
    applyPresetCss(preset, getAccent());
    applyBackground(getBackground());
    applyFontSize(getFontSize());
  }, []);
}

// ---- 切风格档：应用 CSS 变量 + 重放背景（背景档可能残留上个档的渐变/图片）----
export function applyPresetChange(id: PresetId, accent?: string | null) {
  applyPresetCss(PRESETS[id], accent ?? getAccent());
  applyBackground(getBackground());
}
