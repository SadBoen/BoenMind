/**
 * 背景特效层（2026-08-16）：独立于皮肤/壁纸的动画层，叠加在壁纸之上。
 * - EffectWave：半透明蓝色波纹（透明 canvas 直接叠加，实色 rgba 填充），
 *   30fps 动画，全局时钟 performance.now()/1000——与界面/挂载无关，多界面速度一致。
 * - 特效与壁纸解耦：任何壁纸（渐变/自定义图）都可叠加波浪动画。
 * - 以后新增特效（礼花/微风等）：按 EffectWave 模式新写一个组件 + 注册表登记。
 * - reduce-motion 开启时只渲染一帧静态纹理并停止调度（无障碍契约，2026-08-16 修复：
 *   旧实现两分支相同、开启后照常 30fps 动画）；后台标签页/窗口隐藏时暂停循环。
 *
 * 实现说明（2D canvas + 实色直绘，2026-08-16 二次修复）：
 * ① 交接 HANDOFF_BG_EFFECT_ANIMATION：WebGL 帧不被合成器提交（假设 A）→ 2D canvas；
 * ② 2D 版仍不可见——根因是 mix-blend-overlay：浅色壁纸上 overlay 对比度趋近于零
 *    （画了也看不见），且 Chromium 对 blend-mode 元素存在不重绘的已知问题
 *    （issue 503638）。现去掉混合模式、改普通透明 canvas + 实色 rgba 直绘：
 *    无合成怪癖、浅色/深色壁纸上都可见。帧内容仍经 dataset.frames/lastTime 观测。
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

/** 三层正弦丝带（不同波长/速度/相位），实色直绘保证任何壁纸上可见 */
const WAVE_LAYERS: WaveLayer[] = [
  { amp: 0.05, freq: 2.2, speed: 1.1, phase: 0.0, alpha: 0.34, base: 0.28, band: 0.040 },
  { amp: 0.035, freq: 3.4, speed: 1.7, phase: 1.7, alpha: 0.26, base: 0.50, band: 0.032 },
  { amp: 0.065, freq: 1.4, speed: 0.6, phase: 3.1, alpha: 0.20, base: 0.72, band: 0.050 },
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
  ctx.fillStyle = `rgba(70, 125, 235, ${L.alpha})`;
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
  ctx.fillStyle = `rgba(255, 255, 255, ${Math.min(0.25, L.alpha * 0.6)})`;
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

/** 波浪特效层：叠加在壁纸之上（overlay 混合），全局时钟驱动 */
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
      // 帧心跳观测（交接文档实证有效的排查手段）：IAB 截图冻结帧无法验证动画，
      // 改代码后凭 dataset 计数器确认渲染循环在跑
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
