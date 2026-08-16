/**
 * 蓝色波浪壁纸（2026-08-16）：自研 2D canvas 流体波浪。
 *
 * 观感参照 deepseek 官网 join-section 流体背景（dsh-client-ui-aqua 的
 * "Living fluid board" 同源）：深蓝底 + 多层大振幅正弦光带 + 波峰高光。
 *
 * 实现为自研 2D 绘制（多层正弦波 + 渐变填充），不用 WebGL——实测
 * （headless/IAB 等无 GPU Chromium）：WebGL shader 在 GPU 内存正常产帧
 * （readPixels 96% 像素变化）但合成输出不呈现（黑屏），2D canvas 则全环境
 * 呈现且动画正常。2D 全屏 30fps 光带绘制开销极小，无性能顾虑。
 *
 * 壁纸层渲染**静态帧**（time=0 纯纹理）——动画属于背景特效层（effects.tsx
 * EffectWave，独立开关，与壁纸/皮肤解耦；全局时钟统一速度）。
 *
 * 复用：全屏壁纸（FluidWave 组件）、设置页缩略图（mini canvas）、
 * 特效动画层（EffectWave 每帧调用）共用 renderFluid。
 */
import { useEffect, useRef } from "react";
import { useTheme } from "next-themes";

/** 单条光带：上下两条同相正弦曲线围成的粗壮波浪带（观感参数） */
interface WaveBand {
  /** 振幅（画布高比例） */
  amp: number;
  /** 波长（横向周期数） */
  freq: number;
  /** 相速度（rad/s） */
  speed: number;
  /** 初始相位 */
  phase: number;
  /** 带中心 y（画布高比例） */
  base: number;
  /** 带宽（画布高比例） */
  band: number;
  /** 斜向倾斜（沿 x 全宽偏移的画布高比例，正 = 左低右高） */
  tilt: number;
}

/** 三条粗壮光带覆盖全屏（上中下），不同波长/速度/相位——流动层次感 */
const BANDS: WaveBand[] = [
  { amp: 0.10, freq: 1.4, speed: 0.55, phase: 0.0, base: 0.24, band: 0.34, tilt: 0.02 },
  { amp: 0.09, freq: 1.9, speed: 0.85, phase: 2.1, base: 0.52, band: 0.36, tilt: 0.07 },
  { amp: 0.12, freq: 1.2, speed: 0.40, phase: 4.2, base: 0.80, band: 0.38, tilt: -0.05 },
];

/** 背景雾层：低频大振幅半透明正弦晕（慢速漂移），增加流体通透感 */
const HAZE: WaveBand = { amp: 0.22, freq: 0.6, speed: 0.25, phase: 1.4, base: 0.5, band: 1.1, tilt: 0 };

/** 亮/暗两套配色：bg 垂直渐变 + 光带顶/底色 + 高光 */
const PALETTES = {
  light: {
    bg: ["#bfdbfe", "#5e9ff2"],
    band: ["#ffffff", "#7cb3f7"],
    haze: "rgba(255,255,255,0.55)",
    glow: "rgba(255,255,255,0.75)",
  },
  dark: {
    bg: ["#0a2350", "#0d4aa0"],
    band: ["#e8f2ff", "#1a5cbd"],
    haze: "rgba(96,165,250,0.32)",
    glow: "rgba(232,242,255,0.8)",
  },
} as const;

/** 单条光带：顶/底两条同相正弦曲线闭合，垂直渐变（波峰亮白 → 带底深蓝） */
function drawBand(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  t: number,
  B: WaveBand,
  topColor: string,
  bottomColor: string,
  glow: string,
) {
  const yAt = (x: number, o: number) =>
    (B.base + o) * h +
    B.tilt * (x / w - 0.5) * h +
    Math.sin((x / w) * B.freq * Math.PI * 2 + t * B.speed + B.phase) * B.amp * h;
  const step = Math.max(4, Math.round(w / 220));

  // 闭合光带路径：顶曲线 → 底曲线回程
  ctx.beginPath();
  ctx.moveTo(0, yAt(0, -B.band / 2));
  for (let x = 0; x <= w; x += step) ctx.lineTo(x, yAt(x, -B.band / 2));
  for (let x = w; x >= 0; x -= step) ctx.lineTo(x, yAt(x, B.band / 2));
  ctx.closePath();

  const grad = ctx.createLinearGradient(
    0,
    (B.base - B.amp - B.band / 2) * h,
    0,
    (B.base + B.amp + B.band / 2) * h,
  );
  grad.addColorStop(0, topColor);
  grad.addColorStop(1, bottomColor);
  ctx.fillStyle = grad;
  ctx.fill();

  // 波峰高光描边（波纹"亮脊"）
  ctx.beginPath();
  ctx.moveTo(0, yAt(0, -B.band / 2));
  for (let x = 0; x <= w; x += step) ctx.lineTo(x, yAt(x, -B.band / 2));
  ctx.strokeStyle = glow;
  ctx.lineWidth = Math.max(1.5, h * 0.008);
  ctx.stroke();
}

/** 渲染一帧流体（全屏壁纸/特效/设置页缩略图共用；time 秒，壁纸传 0 静态） */
export function renderFluid(canvas: HTMLCanvasElement, dark: boolean, time = 0) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const w = canvas.width;
  const h = canvas.height;
  const pal = dark ? PALETTES.dark : PALETTES.light;
  const t = time * 0.8; // 全局慢速（流动舒缓不晃眼）

  // 底：垂直渐变
  const bg = ctx.createLinearGradient(0, 0, 0, h);
  bg.addColorStop(0, pal.bg[0]);
  bg.addColorStop(1, pal.bg[1]);
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, w, h);

  // 背景雾层（先画，光带压在其上）
  ctx.save();
  ctx.globalAlpha = 0.55;
  drawBand(ctx, w, h, t, HAZE, pal.haze, pal.haze, "rgba(255,255,255,0)");
  ctx.restore();

  // 三条主光带（下层的先画，上层覆盖形成层次）
  for (const B of BANDS) {
    drawBand(ctx, w, h, t, B, pal.band[0], pal.band[1], pal.glow);
  }
}

/** 全屏流体壁纸组件（静态纹理；动画由背景特效层提供，见 effects.tsx） */
export function FluidWave() {
  const ref = useRef<HTMLCanvasElement>(null);
  const { resolvedTheme } = useTheme();

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const resize = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
      canvas.width = Math.max(1, Math.round(canvas.clientWidth * dpr));
      canvas.height = Math.max(1, Math.round(canvas.clientHeight * dpr));
      renderFluid(canvas, resolvedTheme === "dark", 0);
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    return () => ro.disconnect();
  }, [resolvedTheme]);

  return <canvas ref={ref} className="h-full w-full" aria-hidden />;
}
