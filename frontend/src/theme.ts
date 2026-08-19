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
export type PresetId = "ant" | "cartoon" | "glass" | "dark";
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

export function getPresetId(): PresetId {
  const v = localStorage.getItem(PRESET_KEY);
  if (v === "glass" || v === "dark" || v === "cartoon") return v as PresetId;
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
export function getAccent(): string {
  return localStorage.getItem(ACCENT_KEY) || "#4a7dff";
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
    label: "Ant 默认",
    desc: "antd 经典蓝白红（浅色亮底）",
    antd: {
      algorithm: antdTheme.defaultAlgorithm,
      token: {
        colorPrimary: "#1677ff",
        colorError: "#ff4d4f",
        colorBgBase: "#f5f6fa",
        colorBgContainer: "#ffffff",
        colorBgElevated: "#ffffff",
        colorBorder: "#d9dce3",
        colorTextBase: "#1f2430",
        colorTextSecondary: "#6b7385",
        borderRadius: 6,
        fontSize: 14,
      },
    },
    cssVars: {
      "--bm-bg": "#f5f6fa",
      "--bm-bg-2": "#ffffff",
      "--bm-bg-3": "#eef0f4",
      "--bm-border": "#d9dce3",
      "--bm-fg": "#1f2430",
      "--bm-fg-dim": "#6b7385",
      "--bm-accent": "#1677ff",
      "--bm-accent-2": "#4096ff",
      "--bm-danger": "#ff4d4f",
      "--bm-radius": "8px",
    },
  },
  cartoon: {
    id: "cartoon",
    label: "卡通",
    desc: "官网卡通风：暖米底 + 墨绿主色 + 大圆角（手账贴纸感）",
    antd: {
      algorithm: antdTheme.defaultAlgorithm, // 浅色
      token: {
        colorPrimary: "#2C4A47",
        colorError: "#D14545",
        colorSuccess: "#5A8A7A",
        colorBgBase: "#E8DDC9",
        colorBgContainer: "#FAF4E7",
        colorBgElevated: "#FFFDF6",
        colorBorder: "#C9B89A",
        colorTextBase: "#2C4A47",
        colorTextSecondary: "#7A6A50",
        borderRadius: 16,
        borderRadiusLG: 24,
        borderRadiusSM: 12,
        controlHeight: 40,
        fontSize: 14,
      },
      components: {
        Tag: { borderRadiusSM: 9999, defaultBg: "#FAF4E7" },
        Button: { borderRadius: 18, controlHeight: 38, primaryColor: "#fff" },
        Input: { borderRadius: 14 },
        Segmented: { itemSelectedBg: "#2C4A47", itemSelectedColor: "#fff", borderRadius: 9999 },
      },
    },
    cssVars: {
      "--bm-bg": "#E8DDC9",
      "--bm-bg-2": "#FAF4E7",
      "--bm-bg-3": "#EFE3CE",
      "--bm-border": "#C9B89A",
      "--bm-fg": "#2C4A47",
      "--bm-fg-dim": "#7A6A50",
      "--bm-accent": "#2C4A47",
      "--bm-accent-2": "#5A8A7A",
      "--bm-danger": "#D14545",
      "--bm-radius": "16px",
    },
    className: "preset-cartoon",
  },
  glass: {
    id: "glass",
    label: "玻璃",
    desc: "半透明面板 + 背景模糊（紫）",
    antd: {
      algorithm: antdTheme.darkAlgorithm,
      token: {
        colorPrimary: "#a78bfa",
        colorBgBase: "#0e1320",
        colorBgContainer: "rgba(22, 29, 46, 0.6)",
        colorBgElevated: "rgba(29, 38, 57, 0.72)",
        colorBorder: "rgba(167, 139, 250, 0.2)",
        colorTextBase: "#e9ecf4",
        colorTextSecondary: "#9aa3b5",
        borderRadius: 14,
        fontSize: 14,
      },
    },
    cssVars: {
      "--bm-bg": "#0e1320",
      "--bm-bg-2": "rgba(22, 29, 46, 0.6)",
      "--bm-bg-3": "rgba(29, 38, 57, 0.72)",
      "--bm-border": "rgba(167, 139, 250, 0.2)",
      "--bm-fg": "#e9ecf4",
      "--bm-fg-dim": "#9aa3b5",
      "--bm-accent": "#a78bfa",
      "--bm-accent-2": "#8b5cf6",
      "--bm-danger": "#fb7185",
      "--bm-radius": "14px",
    },
    className: "preset-glass",
  },
  dark: {
    id: "dark",
    label: "暗黑",
    desc: "极致深黑，青色点缀",
    antd: {
      algorithm: antdTheme.darkAlgorithm,
      token: {
        colorPrimary: "#22d3ee",
        colorBgBase: "#08090c",
        colorBgContainer: "#0f1014",
        colorBgElevated: "#181a20",
        colorBorder: "#1f2229",
        colorTextBase: "#e6e8ec",
        colorTextSecondary: "#7c8291",
        borderRadius: 6,
        fontSize: 14,
      },
    },
    cssVars: {
      "--bm-bg": "#08090c",
      "--bm-bg-2": "#0f1014",
      "--bm-bg-3": "#181a20",
      "--bm-border": "#1f2229",
      "--bm-fg": "#e6e8ec",
      "--bm-fg-dim": "#7c8291",
      "--bm-accent": "#22d3ee",
      "--bm-accent-2": "#0ea5e9",
      "--bm-danger": "#fb7185",
      "--bm-radius": "6px",
    },
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
export function applyPresetCss(preset: Preset, accent: string) {
  // dockview 桥接变量
  const vars: Record<string, string> = {
    ...preset.cssVars,
    "--dv-background-color": preset.cssVars["--bm-bg"],
    "--dv-group-view-background-color": preset.cssVars["--bm-bg"],
    "--dv-tabs-and-actions-container-background-color": preset.cssVars["--bm-bg-2"],
    "--dv-tab-active-background-color": preset.cssVars["--bm-bg-3"],
    "--dv-tab-inactive-background-color": preset.cssVars["--bm-bg-2"],
    "--dv-tab-active-color": preset.cssVars["--bm-fg"],
    "--dv-tab-inactive-color": preset.cssVars["--bm-fg-dim"],
    "--dv-tabs-container-background-color": preset.cssVars["--bm-bg-2"],
    "--dv-active-sash-color": preset.cssVars["--bm-accent"],
    "--dv-separator-border": preset.cssVars["--bm-border"],
  };
  // 用户自定义强调色覆盖 accent（含 80% 透明变体）
  vars["--bm-accent"] = accent;
  vars["--bm-accent-2"] = accent + "cc";
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
export function applyPresetChange(id: PresetId, accent?: string) {
  applyPresetCss(PRESETS[id], accent ?? getAccent());
  applyBackground(getBackground());
}
