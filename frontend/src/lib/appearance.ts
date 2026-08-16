/**
 * 界面外观设置（外观设置页使用；App 挂载时读取应用）。
 * 与组件分离：被多处 import 的工具函数放独立文件（Fast refresh 纪律）。
 *
 * 字体档位：直接改 html 根字号（非响应式，立即生效）。
 * 强调色/减少动画：html 属性/类（资深模式设置项；状态在 zustand store）。
 * （桌面形态壁纸模板已随桌面壳退役删除，2026-08-16）
 */

/** 字体档位（映射 html 根字号 px；rem 布局全局缩放） */
export const FONT_SCALES = [
  { key: "small", px: 14 },
  { key: "base", px: 16 },
  { key: "large", px: 18 },
] as const;
export type FontScale = (typeof FONT_SCALES)[number]["key"];

/** 读取/应用字体档位（App 挂载时也会调用一次） */
export function fontScale(): FontScale {
  const saved = localStorage.getItem("boenmind.fontScale") as FontScale | null;
  return FONT_SCALES.some((f) => f.key === saved) ? (saved as FontScale) : "base";
}
export function applyFontScale(scale: FontScale) {
  localStorage.setItem("boenmind.fontScale", scale);
  const px = FONT_SCALES.find((f) => f.key === scale)?.px ?? 16;
  document.documentElement.style.fontSize = `${px}px`;
}

/** 强调色（资深外观）：默认 = 当前黑白主题；其余覆盖 --primary 主色（明暗两套） */
export const ACCENTS = [
  { key: "default", labelKey: "settings.appearance.accent.default" },
  { key: "violet", labelKey: "settings.appearance.accent.violet" },
  { key: "blue", labelKey: "settings.appearance.accent.blue" },
  { key: "green", labelKey: "settings.appearance.accent.green" },
  { key: "orange", labelKey: "settings.appearance.accent.orange" },
  { key: "pink", labelKey: "settings.appearance.accent.pink" },
] as const;
export type Accent = (typeof ACCENTS)[number]["key"];

export function applyAccent(accent: Accent) {
  document.documentElement.dataset.accent = accent === "default" ? "" : accent;
}

/** 减少动画：html.reduce-motion 下全局动画/过渡近零时长 */
export function applyReduceMotion(enabled: boolean) {
  document.documentElement.classList.toggle("reduce-motion", enabled);
}
