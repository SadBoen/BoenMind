/**
 * 界面外观的形态专属设置（外观设置页使用；App/Desktop 挂载时读取应用）。
 * 与组件分离：被多处 import 的工具函数放独立文件（Fast refresh 纪律）。
 *
 * 字体档位：直接改 html 根字号（非响应式，立即生效）。
 * 壁纸模板：状态在 zustand store（Desktop 需响应式重渲染），本文件只留常量。
 */

/** 软件形态字体档位（映射 html 根字号 px；rem 布局全局缩放） */
export const FONT_SCALES = [
  { key: "small", px: 14 },
  { key: "base", px: 16 },
  { key: "large", px: 18 },
] as const;
export type FontScale = (typeof FONT_SCALES)[number]["key"];

/** 桌面形态壁纸模板（.desktop-wallpaper 的修饰类；状态见 app-store） */
export const WALLPAPERS = [
  { key: "starry", labelKey: "settings.appearance.wallpaper.starry" },
  { key: "aurora", labelKey: "settings.appearance.wallpaper.aurora" },
] as const;
export type Wallpaper = (typeof WALLPAPERS)[number]["key"];

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
