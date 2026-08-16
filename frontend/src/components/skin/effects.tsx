/**
 * 背景特效层（2026-08-16）：独立于皮肤/壁纸的动画层，叠加在壁纸之上。
 * - EffectWave：蓝色波纹动画（2D canvas 全屏多组正弦丝带），30fps。
 *
 * 实现说明（git 证据链 + 环境实测，2026-08-16 最终定版）：
 * ① 7dcf783 初版 WebGL（alpha:true + mix-blend-overlay + setTimeout）：静态纹理
 *    可呈现但不流动（HANDOFF_BG_EFFECT_ANIMATION 交接）；
 * ② ba2d1df 时代 FluidWave 动画版（WebGL alpha:false + rAF）曾像素级验证有效
 *    （"实测 1.2s 内 73.6% 像素变化"），但 7dcf783 静态化后动画归特效层；
 * ③ 恢复 WebGL 机制后纯红 shader 测试：**WebGL drawing buffer 在当前环境
 *    （WebView2 系）完全不呈现**（背景 CSS 与 2D canvas 均正常，唯独 WebGL 空白）——
 *    而 2D canvas 的呈现路径已被像素级验证（实色深蓝可见）。
 * → 定版：2D canvas 全屏波纹（呈现可靠 + 无 GPU 依赖），实色直绘保证可见。
 *
 * - reduce-motion 开启时只渲染一帧静态纹理并停止调度（无障碍契约）；
 * - 后台标签页/窗口隐藏时暂停循环（还原时补一帧）；
 * - dataset.frames/lastTime 帧心跳（交接文档实证有效的观测手段）。
 */
import { useEffect, useRef } from "react";
import { useAppStore } from "@/stores/app-store";

interface WaveLayer {
  /** 振幅（占画布高比例） */
  amp: number;
  /** 波长（周期数） */
  freq: number;
  /** 流速（rad/s） */
  speed: number;
  /** 初始相位 */
  phase: number;
  /** 填充透明度 */
  alpha: number;
  /** 垂直基准位置（画布高比例） */
  base: number;
  /** 丝带厚度（画布高比例） */
  band: number;
}

/** 六组正弦丝带覆盖全屏（不同波长/速度/相位），实色直绘保证任何壁纸上可见 */
const WAVE_LAYERS: WaveLayer[] = [
  { amp: 0.06, freq: 1.6, speed: 0.8, phase: 0.0, alpha: 0.40, base: 0.10, band: 0.050 },
  { amp: 0.045, freq: 2.4, speed: 1.2, phase: 1.2, alpha: 0.32, base: 0.27, band: 0.042 },
  { amp: 0.055, freq: 1.9, speed: 0.6, phase: 2.4, alpha: 0.28, base: 0.44, band: 0.048 },
  { amp: 0.040, freq: 3.0, speed: 1.5, phase: 3.3, alpha: 0.30, base: 0.60, band: 0.038 },
  { amp: 0.060, freq: 1.4, speed: 0.9, phase: 4.5, alpha: 0.26, base: 0.76, band: 0.052 },
  { amp: 0.035, freq: 2.6, speed: 1.0, phase: 5.6, alpha: 0.24, base: 0.90, band: 0.034 },
];

/** 单条正弦丝带：上下两条同相正弦曲线围成闭合路径 */
function drawRibbon(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  time: number,
  L: WaveLayer,
) {
  const cy = (x: number, offset: number) =>
    (L.base + offset) * height +
    Math.sin((x / width) * L.freq * Math.PI * 2 + time * L.speed + L.phase) * L.amp * height;
  const step = Math.max(6, Math.round(width / 160));
  ctx.beginPath();
  ctx.moveTo(0, cy(0, 0));
  for (let x = 0; x <= width; x += step) {
    ctx.lineTo(x, cy(x, 0));
  }
  for (let x = width; x >= 0; x -= step) {
    ctx.lineTo(x, cy(x, L.band));
  }
  ctx.closePath();
  ctx.fillStyle = `rgba(60, 115, 230, ${L.alpha})`;
  ctx.fill();
  // 丝带顶缘高光（波纹"亮脊"）
  ctx.beginPath();
  ctx.moveTo(0, cy(0, 0));
  for (let x = 0; x <= width; x += step) {
    ctx.lineTo(x, cy(x, 0));
  }
  for (let x = width; x >= 0; x -= step) {
    ctx.lineTo(x, cy(x, Math.max(0.004, L.band * 0.2)));
  }
  ctx.closePath();
  ctx.fillStyle = `rgba(255, 255, 255, ${Math.min(0.25, L.alpha * 0.5)})`;
  ctx.fill();
}

function renderWave2D(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  time: number,
) {
  ctx.clearRect(0, 0, width, height);
  const t = time * 0.7; // 特效速度（独立于壁纸）
  for (const L of WAVE_LAYERS) {
    drawRibbon(ctx, width, height, t, L);
  }
}

/** 蓝色波纹特效层：30fps 动画（reduce-motion 静态帧；后台暂停） */
export function EffectWave() {
  const ref = useRef<HTMLCanvasElement>(null);
  const reduceMotion = useAppStore((s) => s.reduceMotion);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    // setTimeout 驱动（33ms ≈ 30fps）：rAF 在后台/离屏标签页会被 Chromium 暂停，
    // 特效将完全静止；setTimeout 后台只节流到 1s 仍会流动。前台行为一致。
    let timer = 0;

    const render = () => {
      // 全局时钟：所有特效/界面共用同一时间源，速度天然一致
      renderWave2D(ctx, canvas.width, canvas.height, performance.now() / 1000);
      // 帧心跳观测（交接文档实证有效的排查手段）
      canvas.dataset.frames = String(Number(canvas.dataset.frames ?? 0) + 1);
      canvas.dataset.lastTime = String(Math.round(performance.now()));
    };
    const loop = () => {
      timer = window.setTimeout(loop, 33);
      render();
    };
    const resize = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
      canvas.width = Math.max(1, Math.round(canvas.clientWidth * dpr));
      canvas.height = Math.max(1, Math.round(canvas.clientHeight * dpr));
      render();
    };
    resize();
    if (!reduceMotion) {
      timer = window.setTimeout(loop, 33);
    }
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    // 后台暂停（visibilitychange）：省电 + 还原时补一帧
    const onVis = () => {
      if (document.hidden) {
        clearTimeout(timer);
        timer = 0;
      } else if (!reduceMotion && !timer) {
        render();
        timer = window.setTimeout(loop, 33);
      }
    };
    document.addEventListener("visibilitychange", onVis);
    return () => {
      clearTimeout(timer);
      ro.disconnect();
      document.removeEventListener("visibilitychange", onVis);
    };
  }, [reduceMotion]);

  return <canvas ref={ref} className="h-full w-full" aria-hidden />;
}
