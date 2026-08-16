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

const SKIN_KEY = "boenmind.skin";
const SKIN_PARAMS_KEY = "boenmind.skin.params";
const SKIN_BG_KEY = "boenmind.skin.background";
const SKIN_AUTO_KEY = "boenmind.skin.auto";

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

/** 写入参数 CSS 变量（--skin-<key>）；缺省参数不写，走皮肤 CSS 的 var() 默认值 */
export function applySkinParams(id: SkinId, params: SkinParams) {
  for (const p of skinById(id).params) {
    const v = params[p.key] ?? p.default;
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
  const blur = Math.round(10 + delta * 14); // 10px ~ 24px
  return { alpha, blur };
}
