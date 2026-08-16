/**
 * 皮肤工具（外观设置页使用；App 挂载时恢复）。
 * 机制：html 挂 data-skin="<id>" 启用皮肤 CSS；参数写为 --skin-<key> CSS 变量；
 * 背景图存 localStorage（本地文件压缩为 max 1600px JPEG，URL 直存）。
 * 自动配色：canvas 取样图片主色 → 色调直接采用，玻璃不透明度/模糊按亮度推导
 * （极端亮暗图自动调实调糊，可读性优先）；全部可手动覆盖。
 */
import { SKINS, type SkinId, type SkinParams } from "@/skins";

/** 背景图：data = 本地压缩后的 dataURL；url = 外部图片直链 */
export interface SkinBackground {
  kind: "data" | "url";
  value: string;
}

/**
 * 内置预设壁纸（2026-08-16）：Aqua 插件观感自研版。
 * gradient = CSS 纯渐变（零体积）；fluid = WebGL 静态流体波浪（蓝色波浪款）。
 * 亮暗两套渐变 + 推荐色调（选中时写入 --skin-hue，与自动配色同一通道）。
 * 优先级：自定义背景图 > 预设壁纸 > 默认色调渐变。
 */
export interface PresetWallpaper {
  id: string;
  nameKey: string;
  /** gradient = CSS 渐变（css/darkCss 生效）；fluid = WebGL 波浪（FluidWave 渲染） */
  kind: "gradient" | "fluid";
  /** 亮色主题渐变（fluid 款不用） */
  css?: string;
  /** 暗色主题渐变（fluid 款不用） */
  darkCss?: string;
  /** 推荐色调（0-360，应用时写入 hue 参数） */
  hue: number;
}

export const PRESET_WALLPAPERS: PresetWallpaper[] = [
  {
    id: "aqua",
    nameKey: "settings.appearance.skin.wallpaper.aqua",
    kind: "gradient",
    css: "radial-gradient(120% 100% at 15% 10%, rgba(20,184,166,0.55) 0%, transparent 55%), radial-gradient(110% 110% at 90% 85%, rgba(59,130,246,0.5) 0%, transparent 60%), linear-gradient(135deg, #ecfeff 0%, #cffafe 45%, #bfdbfe 100%)",
    darkCss: "radial-gradient(120% 100% at 15% 10%, rgba(13,148,136,0.6) 0%, transparent 55%), radial-gradient(110% 110% at 90% 85%, rgba(37,99,235,0.55) 0%, transparent 60%), linear-gradient(135deg, #042f2e 0%, #083344 50%, #172554 100%)",
    hue: 190,
  },
  {
    id: "bluewave",
    nameKey: "settings.appearance.skin.wallpaper.bluewave",
    kind: "fluid",
    hue: 220,
  },
  {
    id: "sunset",
    nameKey: "settings.appearance.skin.wallpaper.sunset",
    kind: "gradient",
    css: "radial-gradient(120% 100% at 20% 20%, rgba(251,146,60,0.6) 0%, transparent 55%), radial-gradient(110% 110% at 85% 80%, rgba(236,72,153,0.45) 0%, transparent 60%), linear-gradient(135deg, #fff7ed 0%, #fed7aa 50%, #fbcfe8 100%)",
    darkCss: "radial-gradient(120% 100% at 20% 20%, rgba(234,88,12,0.55) 0%, transparent 55%), radial-gradient(110% 110% at 85% 80%, rgba(190,24,93,0.5) 0%, transparent 60%), linear-gradient(135deg, #431407 0%, #7c2d12 55%, #4c0519 100%)",
    hue: 25,
  },
  {
    id: "aurora",
    nameKey: "settings.appearance.skin.wallpaper.aurora",
    kind: "gradient",
    css: "radial-gradient(120% 100% at 25% 15%, rgba(52,211,153,0.5) 0%, transparent 55%), radial-gradient(110% 110% at 80% 85%, rgba(168,85,247,0.4) 0%, transparent 60%), linear-gradient(135deg, #f0fdf4 0%, #d1fae5 45%, #ede9fe 100%)",
    darkCss: "radial-gradient(120% 100% at 25% 15%, rgba(16,185,129,0.5) 0%, transparent 55%), radial-gradient(110% 110% at 80% 85%, rgba(147,51,234,0.5) 0%, transparent 60%), linear-gradient(135deg, #022c22 0%, #064e3b 50%, #2e1065 100%)",
    hue: 155,
  },
  {
    id: "nebula",
    nameKey: "settings.appearance.skin.wallpaper.nebula",
    kind: "gradient",
    css: "radial-gradient(120% 100% at 15% 20%, rgba(129,140,248,0.5) 0%, transparent 55%), radial-gradient(110% 110% at 85% 75%, rgba(244,114,182,0.4) 0%, transparent 60%), linear-gradient(135deg, #eef2ff 0%, #e0e7ff 50%, #fce7f3 100%)",
    darkCss: "radial-gradient(120% 100% at 15% 20%, rgba(99,102,241,0.55) 0%, transparent 55%), radial-gradient(110% 110% at 85% 75%, rgba(219,39,119,0.45) 0%, transparent 60%), linear-gradient(135deg, #1e1b4b 0%, #312e81 50%, #500724 100%)",
    hue: 250,
  },
];

export function wallpaperById(id: string | null): PresetWallpaper | null {
  return PRESET_WALLPAPERS.find((w) => w.id === id) ?? null;
}

const SKIN_KEY = "boenmind.skin";
const SKIN_PARAMS_KEY = "boenmind.skin.params";
const SKIN_BG_KEY = "boenmind.skin.background";
const SKIN_AUTO_KEY = "boenmind.skin.auto";
const SKIN_WALLPAPER_KEY = "boenmind.skin.wallpaper";

/** 皮肤列表按 id 索引（参数声明/默认值查询） */
export function skinById(id: string) {
  return SKINS.find((s) => s.id === id) ?? SKINS[0];
}

// ── 皮肤 id ──────────────────────────────────────────────

export function loadSkinId(): SkinId {
  const saved = localStorage.getItem(SKIN_KEY);
  return SKINS.some((s) => s.id === saved) ? (saved as SkinId) : "classic";
}

/** 启用皮肤：挂 data-skin 属性 + 写入参数变量（classic = 去掉属性，完整还原） */
export function applySkin(id: SkinId, params: SkinParams) {
  document.documentElement.dataset.skin = id === "classic" ? "" : id;
  applySkinParams(id, params);
}

// ── 参数 ─────────────────────────────────────────────────

export function loadSkinParams(id: SkinId): SkinParams {
  try {
    const raw = localStorage.getItem(SKIN_PARAMS_KEY);
    const all = raw ? (JSON.parse(raw) as Record<string, SkinParams>) : {};
    return all[id] ?? {};
  } catch {
    return {};
  }
}

export function saveSkinParams(id: SkinId, params: SkinParams) {
  const raw = localStorage.getItem(SKIN_PARAMS_KEY);
  const all = raw ? (JSON.parse(raw) as Record<string, SkinParams>) : {};
  all[id] = params;
  localStorage.setItem(SKIN_PARAMS_KEY, JSON.stringify(all));
}

/** 写入参数 CSS 变量（--skin-<key>）；缺省参数不写，走皮肤 CSS 的 var() 默认值。
 *  单位归一化：percent 参数（0-100）归一为 0-1 alpha 语义；hue/blur 写原始数值，
 *  blur 单位在皮肤 CSS 里 calc(* 1px) 补上。 */
export function applySkinParams(id: SkinId, params: SkinParams) {
  for (const p of skinById(id).params) {
    let v = params[p.key] ?? p.default;
    if (p.format === "percent") v /= 100;
    document.documentElement.style.setProperty(`--skin-${p.key}`, String(v));
  }
}

/** 参数默认值兜底（设置页滑杆回显用） */
export function skinParamValue(id: SkinId, params: SkinParams, key: string): number {
  return params[key] ?? skinById(id).params.find((p) => p.key === key)?.default ?? 0;
}

// ── 背景图 ───────────────────────────────────────────────

export function loadSkinBackground(): SkinBackground | null {
  try {
    const raw = localStorage.getItem(SKIN_BG_KEY);
    if (!raw) return null;
    const bg = JSON.parse(raw) as SkinBackground;
    return bg && (bg.kind === "data" || bg.kind === "url") && bg.value ? bg : null;
  } catch {
    return null;
  }
}

export function saveSkinBackground(bg: SkinBackground | null) {
  if (bg) localStorage.setItem(SKIN_BG_KEY, JSON.stringify(bg));
  else localStorage.removeItem(SKIN_BG_KEY);
}

/** 内置预设壁纸（preset id；与自定义背景图互斥，选预设时清自定义） */
export function loadSkinWallpaper(): string | null {
  const saved = localStorage.getItem(SKIN_WALLPAPER_KEY);
  return saved && PRESET_WALLPAPERS.some((w) => w.id === saved) ? saved : null;
}

export function saveSkinWallpaper(id: string | null) {
  if (id) localStorage.setItem(SKIN_WALLPAPER_KEY, id);
  else localStorage.removeItem(SKIN_WALLPAPER_KEY);
}

/** 本地图片压缩为 dataURL（max 1600px JPEG 0.82，防 localStorage 配额） */
export function compressImageFile(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => {
      URL.revokeObjectURL(url);
      const max = 1600;
      const scale = Math.min(1, max / Math.max(img.width, img.height));
      const w = Math.max(1, Math.round(img.width * scale));
      const h = Math.max(1, Math.round(img.height * scale));
      const canvas = document.createElement("canvas");
      canvas.width = w;
      canvas.height = h;
      canvas.getContext("2d")!.drawImage(img, 0, 0, w, h);
      resolve(canvas.toDataURL("image/jpeg", 0.82));
    };
    img.onerror = () => {
      URL.revokeObjectURL(url);
      reject(new Error("image load failed"));
    };
    img.src = url;
  });
}

// ── 取色与自动配色 ───────────────────────────────────────

export interface ImageSample {
  /** 主色调相角（0-360） */
  hue: number;
  /** 亮度 0-1 */
  lightness: number;
}

function rgbToHsl(r: number, g: number, b: number) {
  r /= 255;
  g /= 255;
  b /= 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return { h: 0, s: 0, l };
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
  else if (max === g) h = ((b - r) / d + 2) / 6;
  else h = ((r - g) / d + 4) / 6;
  return { h: h * 360, s, l };
}

/**
 * 取样图片主色：缩小到 32×32 画布读取像素，中心区域加权平均。
 * 外部 URL 图片若服务器不允许跨域读取（canvas 被污染）会抛错，
 * 调用方应捕获并降级（保留当前参数）。
 */
export async function sampleImage(src: string): Promise<ImageSample> {
  const img = new Image();
  img.crossOrigin = "anonymous";
  img.src = src;
  await img.decode().catch(() => new Promise<void>((res, rej) => {
    img.onload = () => res();
    img.onerror = () => rej(new Error("image load failed"));
  }));
  const size = 32;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d", { willReadFrequently: true })!;
  ctx.drawImage(img, 0, 0, size, size);
  const data = ctx.getImageData(0, 0, size, size).data;
  const c = size / 2;
  let r = 0;
  let g = 0;
  let b = 0;
  let n = 0;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      // 中心权重：高斯近似（边缘对主色影响小）
      const w = Math.exp(-((x - c) ** 2 + (y - c) ** 2) / 40);
      const i = (y * size + x) * 4;
      r += data[i] * w;
      g += data[i + 1] * w;
      b += data[i + 2] * w;
      n += w;
    }
  }
  const { h, l } = rgbToHsl(r / n, g / n, b / n);
  return { hue: h, lightness: l };
}

/**
 * 按图片亮度推导玻璃参数：极端亮/暗图 → 更实更糊（保证文字可读性），
 * 中等亮度 → 更通透。色调由主色直接给定（调用方写入 hue）。
 */
export function autoGlassParams(sample: ImageSample): { alpha: number; blur: number } {
  const delta = Math.abs(sample.lightness - 0.5); // 0 ~ 0.5
  const alpha = Math.round(55 + delta * 40); // 55% ~ 75%
  const blur = Math.round(14 + delta * 20); // 14px ~ 24px
  return { alpha, blur };
}
