/**
 * 背景特效层（2026-08-16）：独立于皮肤/壁纸的动画层，叠加在壁纸之上。
 * - EffectWave：整屏蓝色流体波浪（2D canvas 多层正弦光带），30fps 动画。
 *
 * 实现说明（实测证据链，2026-08-16）：
 * WebGL 方案在无 GPU Chromium（headless / IAB）下虽正常产帧（readPixels
 * 96% 像素变化）但合成输出不呈现（黑屏）——2D canvas 则全环境呈现且
 * 动画正常。故定版为 2D 绘制（renderFluid 共用，观感对标 deepseek 官网
 * 流体：深蓝底 + 粗壮正弦光带 + 波峰高光），特效层自带底色盖过壁纸，
 * 选择"蓝色波纹"即得到完整流体画面。
 *
 * - reduce-motion 开启时只渲染一帧静态纹理并停止调度（无障碍契约）；
 * - 后台标签页/窗口隐藏时暂停循环（还原时补一帧）；
 * - dataset.frames/lastTime 帧心跳（观测手段）。
 */
import { useEffect, useRef } from "react";
import { useTheme } from "next-themes";
import { useAppStore } from "@/stores/app-store";
import { renderFluid } from "@/components/skin/FluidWave";

/** 蓝色波纹特效层：整屏流体 30fps 动画（reduce-motion 静态帧；后台暂停） */
export function EffectWave() {
  const ref = useRef<HTMLCanvasElement>(null);
  const { resolvedTheme } = useTheme();
  const reduceMotion = useAppStore((s) => s.reduceMotion);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;

    let raf = 0;
    let last = 0;

    const render = (timeSec: number) => {
      renderFluid(canvas, resolvedTheme === "dark", timeSec);
      // 帧心跳观测
      canvas.dataset.frames = String(Number(canvas.dataset.frames ?? 0) + 1);
      canvas.dataset.lastTime = String(Math.round(performance.now()));
    };

    const loop = (t: number) => {
      raf = requestAnimationFrame(loop);
      // 30fps 节流（低开销动画，避免空转 GPU）
      if (t - last < 33) return;
      last = t;
      render(t / 1000);
    };

    const resize = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
      canvas.width = Math.max(1, Math.round(canvas.clientWidth * dpr));
      canvas.height = Math.max(1, Math.round(canvas.clientHeight * dpr));
      render(reduceMotion ? 0 : last / 1000);
    };
    resize();
    if (!reduceMotion) {
      raf = requestAnimationFrame(loop);
    }
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    // 后台暂停（rAF 在后台标签页本会被 Chromium 暂停；显式处理还原补帧）
    const onVis = () => {
      if (!document.hidden && !reduceMotion) {
        cancelAnimationFrame(raf);
        render(last / 1000);
        raf = requestAnimationFrame(loop);
      }
    };
    document.addEventListener("visibilitychange", onVis);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      document.removeEventListener("visibilitychange", onVis);
    };
  }, [resolvedTheme, reduceMotion]);

  return <canvas ref={ref} className="h-full w-full" aria-hidden />;
}
