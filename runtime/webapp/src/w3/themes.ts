// W3 主题系统(ADR-0014;两级换肤:一级=主题预设,二级=每主题自带设置项,
// 项集不必相同——透明度仅玻璃有,规格 §1/§3/§4)。
// 视觉令牌源 = .tools/mockups 样张(玻璃稿已获用户认可「非常棒」,§4.1);
// 落地 CSS 在 theme.css,本文件只负责:主题注册表(schema)+ 设置值 →
// CSS 变量的应用 + localStorage 持久化。实时预览 = 改一项调一次 apply。
// 裁定(§4):文字大小属全局偏好不入主题(全局在 AppearancePage,存
// bm_prefs);字体可入主题(卡通);背景图按主题开启(仅玻璃)。
import { useEffect } from "react";

export type ThemeFieldValue =
  | string
  | number
  | boolean;

export type ThemeField = {
  key: string;
  label: string;
  hint?: string;
  type: "color" | "range" | "toggle" | "select" | "image";
  default: ThemeFieldValue;
  min?: number;
  max?: number;
  step?: number;
  unit?: string;
  options?: { value: string; label: string }[];
  /** 设置值 → CSS 变量(每次 apply 前已清空旧 inline 变量)。 */
  apply: (v: ThemeFieldValue, set: (k: string, v: string) => void) => void;
};

export type ThemeDef = {
  id: "modern" | "classic" | "cartoon" | "glass";
  label: string;
  desc: string;
  /** 卡片预览色(外观页主题选择卡)。 */
  swatch: { page: string; panel: string; accent: string };
  /** 通用项(主配色)之外的主题专属项,项集按规格 §3 各不相同。 */
  fields: ThemeField[];
};

const hexA = (hex: string, alpha: number) => {
  const n = hex.replace("#", "");
  const r = parseInt(n.slice(0, 2), 16);
  const g = parseInt(n.slice(2, 4), 16);
  const b = parseInt(n.slice(4, 6), 16);
  return `rgba(${r},${g},${b},${alpha})`;
};

/** 通用项:主配色(全部主题都有,规格 §3)。 */
const accentField = (def: string): ThemeField => ({
  key: "accent",
  label: "主配色",
  type: "color",
  default: def,
  apply: (v, set) => {
    const c = typeof v === "string" && v.startsWith("#") ? v : def;
    set("--accent", c);
    set("--accent-raw", c);
    set("--primary", c);
    set("--ring", c);
  },
});

export const THEMES: Record<ThemeDef["id"], ThemeDef> = {
  modern: {
    id: "modern",
    label: "现代",
    desc: "W1 现状:浅色、细边框、专业开发工具风。",
    swatch: { page: "#ffffff", panel: "#fafafa", accent: "#2563eb" },
    fields: [
      accentField("#2563eb"),
      {
        key: "compact",
        label: "紧凑度",
        hint: "消息与列表的垂直密度",
        type: "range",
        min: 0.8,
        max: 1.3,
        step: 0.05,
        default: 1,
        unit: "×",
        apply: (v, set) =>
          set("--density", String(typeof v === "number" ? v : 1)),
      },
    ],
  },
  classic: {
    id: "classic",
    label: "古典",
    desc: "纸墨暖调、衬线字体、传统版式(书房纹理 + 双线框)。",
    swatch: { page: "#f3ecdc", panel: "#ede4cf", accent: "#8a6d3b" },
    fields: [
      accentField("#8a6d3b"),
      {
        key: "serifStrength",
        label: "衬线字体强弱",
        type: "select",
        default: "soft",
        options: [
          { value: "none", label: "无衬线" },
          { value: "soft", label: "衬线(Georgia)" },
          { value: "full", label: "强衬线(宋体系)" },
        ],
        apply: (v, set) => {
          const s = String(v);
          if (s === "none")
            set(
              "--font-ui",
              'system-ui, "Segoe UI", "Microsoft YaHei", sans-serif',
            );
          else if (s === "full")
            set(
              "--font-ui",
              '"Source Han Serif SC", "Noto Serif SC", SimSun, Georgia, serif',
            );
          else
            set(
              "--font-ui",
              'Georgia, "Times New Roman", "Source Han Serif SC", SimSun, serif',
            );
        },
      },
      {
        key: "paperTone",
        label: "纸色深浅",
        type: "range",
        min: 0,
        max: 2,
        step: 1,
        default: 1,
        apply: (v, set) => {
          const tones = [
            { page: "#f7f1e3", panel: "#f1e8d4", hover: "#ece1c8", select: "#efe0bd", border: "#d3c39c" },
            { page: "#f3ecdc", panel: "#ede4cf", hover: "#e7dcc2", select: "#e8d9b4", border: "#c9b98f" },
            { page: "#ede2cb", panel: "#e4d7bc", hover: "#dccfab", select: "#e0cd9f", border: "#b3a06f" },
          ];
          const t = tones[Math.min(2, Math.max(0, Math.round(Number(v))))];
          set("--bg-page", t.page);
          set("--bg-panel", t.panel);
          set("--bg-hover", t.hover);
          set("--bg-select", t.select);
          set("--border", t.border);
          set("--background", t.page);
          set("--card", t.panel);
          set("--popover", t.panel);
          set("--secondary", t.hover);
          set("--muted", t.hover);
          set("--input", t.border);
        },
      },
      {
        key: "leading",
        label: "行距",
        type: "range",
        min: 1.5,
        max: 2.1,
        step: 0.05,
        default: 1.75,
        apply: (v, set) => set("--leading", String(v)),
      },
    ],
  },
  cartoon: {
    id: "cartoon",
    label: "卡通",
    desc: "大圆角、明快配色、活泼氛围。",
    swatch: { page: "#fff8f3", panel: "#fff1e8", accent: "#ff6b9d" },
    fields: [
      accentField("#ff6b9d"),
      {
        key: "radiusScale",
        label: "圆角大小",
        type: "range",
        min: 0.7,
        max: 1.6,
        step: 0.1,
        default: 1,
        unit: "×",
        apply: (v, set) => {
          const s = typeof v === "number" ? v : 1;
          set("--radius", `${Math.round(18 * s)}px`);
          set("--radius-sm", `${Math.round(14 * s)}px`);
        },
      },
      {
        key: "funLevel",
        label: "配色活泼度",
        type: "select",
        default: "normal",
        options: [
          { value: "soft", label: "柔和" },
          { value: "normal", label: "活泼" },
          { value: "wild", label: "缤纷" },
        ],
        apply: (v, set) => {
          const tones = {
            soft: { page: "#fff9f6", panel: "#fff4ee", hover: "#ffeee6", select: "#ffe4ee", border: "#ffd8e6", fg1: "#5a3a4a" },
            normal: { page: "#fff8f3", panel: "#fff1e8", hover: "#ffe9dc", select: "#ffe0ee", border: "#ffc9df", fg1: "#4a2c3f" },
            wild: { page: "#fff5fb", panel: "#ffecf7", hover: "#ffe3f0", select: "#ffd8ec", border: "#ffb8dd", fg1: "#47224d" },
          }[String(v)] ?? { page: "#fff8f3", panel: "#fff1e8", hover: "#ffe9dc", select: "#ffe0ee", border: "#ffc9df", fg1: "#4a2c3f" };
          set("--bg-page", tones.page);
          set("--bg-panel", tones.panel);
          set("--bg-hover", tones.hover);
          set("--bg-select", tones.select);
          set("--border", tones.border);
          set("--fg-1", tones.fg1);
          set("--background", tones.page);
          set("--card", tones.panel);
          set("--popover", tones.panel);
          set("--secondary", tones.hover);
          set("--muted", tones.hover);
          set("--input", tones.border);
          // 活泼度也控制底部色影浓度(卡通专属样式,theme.css 消费)
          set("--cartoon-shadow-a", String(v) === "soft" ? "0.10" : String(v) === "wild" ? "0.26" : "0.18");
        },
      },
      {
        key: "emojiDecor",
        label: "表情装饰",
        hint: "空态与欢迎语带可爱表情",
        type: "toggle",
        default: true,
        apply: (v, set) =>
          set("--emoji-decor", v ? "1" : "0"),
      },
      {
        key: "fontFace",
        label: "文字字体",
        hint: "主题字体可换(§4 裁定)",
        type: "select",
        default: "cartoon",
        options: [
          { value: "cartoon", label: "卡通默认(圆体感)" },
          { value: "round", label: "幼圆" },
          { value: "kai", label: "楷体" },
        ],
        apply: (v, set) => {
          const fonts = {
            cartoon:
              '"Comic Sans MS", "Chalkboard SE", YouYuan, "幼圆", "Microsoft YaHei", sans-serif',
            round: 'YouYuan, "幼圆", "Microsoft YaHei", sans-serif',
            kai: 'KaiTi, "楷体", STKaiti, serif',
          };
          set("--font-ui", fonts[String(v)] ?? fonts.cartoon);
        },
      },
    ],
  },
  glass: {
    id: "glass",
    label: "玻璃",
    desc: "半透明毛玻璃、通透层次(已认可样张:樱花背景 + 花瓣)。",
    swatch: { page: "#cfd8e6", panel: "rgba(255,255,255,.42)", accent: "#8fa3c4" },
    fields: [
      accentField("#5b78a6"),
      {
        key: "alpha",
        label: "透明度",
        hint: "仅玻璃主题有",
        type: "range",
        min: 0.25,
        max: 0.7,
        step: 0.02,
        default: 0.42,
        apply: (v, set) =>
          set("--glass-alpha", String(typeof v === "number" ? v : 0.42)),
      },
      {
        key: "blur",
        label: "模糊强度",
        type: "range",
        min: 6,
        max: 32,
        step: 2,
        default: 16,
        unit: "px",
        apply: (v, set) =>
          set("--glass-blur", `${Number(v) || 16}px`),
      },
      {
        key: "bgImage",
        label: "背景图",
        hint: "留空 = 内置樱花;自定义图保存在本机浏览器",
        type: "image",
        default: "/themes/sakura.jpg",
        apply: (v, set) => {
          const img = typeof v === "string" && v ? v : "/themes/sakura.jpg";
          set(
            "--glass-bg-image",
            `url("${img.replace(/"/g, "%22")}")`,
          );
        },
      },
      {
        key: "dim",
        label: "背景暗化",
        type: "range",
        min: 0,
        max: 0.6,
        step: 0.05,
        default: 0.08,
        apply: (v, set) =>
          set("--glass-dim", String(typeof v === "number" ? v : 0.08)),
      },
    ],
  },
};

export const THEME_ORDER: ThemeDef["id"][] = [
  "modern",
  "classic",
  "cartoon",
  "glass",
];

const THEME_KEY = "bm_theme"; // { theme, settings: { [themeId]: {...} } }

export type ThemeState = {
  theme: ThemeDef["id"];
  settings: Partial<Record<ThemeDef["id"], Record<string, ThemeFieldValue>>>;
};

export function loadThemeState(): ThemeState {
  try {
    const raw = localStorage.getItem(THEME_KEY);
    if (raw) {
      const v = JSON.parse(raw) as ThemeState;
      if (v.theme && THEME_ORDER.includes(v.theme)) return v;
    }
  } catch {
    /* 损坏即回默认 */
  }
  return { theme: "modern", settings: {} };
}

export function saveThemeState(s: ThemeState) {
  localStorage.setItem(THEME_KEY, JSON.stringify(s));
}

/** 全局偏好(§4:文字大小与主题无关)。 */
export function loadFontPref(): number {
  const v = Number(localStorage.getItem("bm_font_size"));
  return v >= 11 && v <= 17 ? v : 13.5;
}

export function saveFontPref(px: number) {
  localStorage.setItem("bm_font_size", String(px));
}

/** 把主题+设置项应用到 <html>(data-theme + inline 变量;先清后写防残留)。 */
export function applyTheme(state: ThemeState) {
  const root = document.documentElement;
  root.setAttribute("data-theme", state.theme);
  for (const k of [...root.style]) {
    if (k.startsWith("--")) root.style.removeProperty(k);
  }
  const def = THEMES[state.theme];
  const saved = state.settings[state.theme] ?? {};
  const set = (k: string, v: string) => root.style.setProperty(k, v);
  for (const f of def.fields) {
    const value = saved[f.key] !== undefined ? saved[f.key] : f.default;
    f.apply(value, set);
  }
  // 卡通表情装饰开关(CSS 按 html[data-emoji] 挂 content)
  if (state.theme === "cartoon") {
    root.setAttribute("data-emoji", saved.emojiDecor === false ? "0" : "1");
  } else {
    root.removeAttribute("data-emoji");
  }
  // 玻璃面板派生色(alpha 参与,accent-raw 供玻璃块 color-mix)
  if (state.theme === "glass") {
    const a = String(saved.alpha ?? 0.42);
    const raw = typeof saved.accent === "string" && saved.accent.startsWith("#") ? saved.accent : "#5b78a6";
    set("--glass-alpha", a);
    set("--accent", hexA(raw, 0.55));
  }
}

/** React 侧:挂载即恢复(重启保持)。 */
export function useThemeBoot() {
  useEffect(() => {
    applyTheme(loadThemeState());
  }, []);
}
