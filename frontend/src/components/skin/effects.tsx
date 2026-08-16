/**
 * 背景特效层（2026-08-16 复开轮）：独立于皮肤/壁纸的动画层，叠加在壁纸之上。
 * - EffectWave：波光粼粼流体（自研 WebGL2 domain-warp 噪声，fluid-gl.ts，
 *   观感参照 deepseek 官网流体）+ 指针扰动（光随鼠标流动）。
 *
 * 三级健壮性（本会话实测教训：rAF 在可见性误报/遮挡误判环境会被整段掐死，
 * 页面仍显示但动画冻结——这正是历代"引擎产帧正常、用户看不见"的病根）：
 *   1) WebGL 流体主层（rAF 30fps，真观感）；
 *   2) 冻结看门狗：setInterval 每秒查帧心跳（rAF 死透时 interval 仍 ≥1Hz
 *      存活），停滞 >2.6s → 淡出 WebGL 层、露出 CSS 流光兜底层；
 *      帧恢复自动切回。切换走 data-frozen 属性 + CSS 过渡，无 React 重渲；
 *   3) WebGL 不可用（无 GPU 环境）→ data-fallback 常驻 CSS 层（合成器
 *      transform/opacity 动画，完全不依赖 JS 帧循环）。
 * - reduceMotion：只渲染一帧静态画面（无障碍契约），无看门狗；
 * - dataset.frames/lastTime 帧心跳保留（观测手段）。
 */
import { useEffect, useRef } from "react";
import { useTheme } from "next-themes";
import { useAppStore } from "@/stores/app-store";
import { attachFluid, FLUID_DARK, FLUID_LIGHT } from "@/components/skin/fluid-gl";
import { cn } from "@/lib/utils";

/** CSS 兜底流光层：柔光斑漂移胀缩（transform/opacity 动画=合成器线程，
 *  rAF 冻结也流动）。平时暂停隐藏，仅 data-frozen / data-fallback 时启用。 */
const FALLBACK_CSS = `
.bw-host { position: absolute; inset: 0; overflow: hidden; }
.bw-fallback { position: absolute; inset: 0; opacity: 0; transition: opacity 1.2s; }
.bw-gl { position: absolute; inset: 0; height: 100%; width: 100%; transition: opacity 1.2s; }
.bw-host[data-frozen] .bw-gl, .bw-host[data-fallback] .bw-gl { opacity: 0; }
.bw-host[data-frozen] .bw-fallback, .bw-host[data-fallback] .bw-fallback { opacity: 1; }
.bw-blob {
  position: absolute; border-radius: 50%; will-change: transform, opacity;
  animation: bw-blob var(--dur) ease-in-out infinite; animation-play-state: paused;
}
.bw-host[data-frozen] .bw-blob, .bw-host[data-fallback] .bw-blob { animation-play-state: running; }
@keyframes bw-blob {
  0%, 100% { transform: translate(0, 0) scale(1); opacity: 1; }
  50% { transform: translate(var(--dx), var(--dy)) scale(1.16); opacity: 0.6; }
}
`;

/** 兜底光斑布局（vw/vh 定位，色为半透明白/蓝白） */
const BLOBS = [
  { left: "6%", top: "12%", width: "44vw", height: "36vh", color: "rgba(255,255,255,0.16)", dx: "10vw", dy: "4vh", dur: "19s" },
  { left: "48%", top: "30%", width: "38vw", height: "30vh", color: "rgba(180,215,255,0.20)", dx: "-8vw", dy: "-6vh", dur: "24s" },
  { left: "20%", top: "58%", width: "50vw", height: "34vh", color: "rgba(255,255,255,0.12)", dx: "7vw", dy: "-5vh", dur: "28s" },
  { left: "60%", top: "64%", width: "34vw", height: "28vh", color: "rgba(140,190,255,0.18)", dx: "-9vw", dy: "5vh", dur: "22s" },
  { left: "34%", top: "6%", width: "30vw", height: "24vh", color: "rgba(255,255,255,0.10)", dx: "-6vw", dy: "6vh", dur: "26s" },
] as const;

/** 蓝色波纹特效层：WebGL 流体（主）+ CSS 流光兜底 + 冻结看门狗 */
export function EffectWave() {
  const { resolvedTheme } = useTheme();
  const reduceMotion = useAppStore((s) => s.reduceMotion);
  const hostRef = useRef<HTMLDivElement>(null);
  const glRef = useRef<HTMLCanvasElement>(null);
  const dark = resolvedTheme === "dark";

  useEffect(() => {
    const host = hostRef.current;
    const canvas = glRef.current;
    if (!host || !canvas) return;

    const handle = attachFluid(canvas, dark ? FLUID_DARK : FLUID_LIGHT, reduceMotion);
    if (!handle) {
      // WebGL 不可用：常驻 CSS 兜底层
      host.dataset.fallback = "1";
      return;
    }
    if (reduceMotion) return; // 静态帧契约：无循环无看门狗

    // 冻结看门狗：帧心跳停滞 >2.6s 判定 rAF 被掐死 → 切 CSS 兜底；恢复自动切回
    let lastFrames = -1;
    let lastAdvance = performance.now();
    let frozen = false;
    const check = () => {
      const f = handle.frames();
      if (f !== lastFrames) {
        lastFrames = f;
        lastAdvance = performance.now();
        if (frozen) {
          frozen = false;
          delete host.dataset.frozen;
        }
      } else if (!frozen && performance.now() - lastAdvance > 2600) {
        frozen = true;
        host.dataset.frozen = "1";
      }
    };
    const timer = window.setInterval(check, 1200);
    return () => {
      window.clearInterval(timer);
      handle.dispose();
    };
  }, [dark, reduceMotion]);

  return (
    <div ref={hostRef} aria-hidden className="bw-host">
      <style>{FALLBACK_CSS}</style>
      {/* CSS 流光兜底层（合成器动画；平时隐藏暂停） */}
      <div
        className="bw-fallback"
        style={{
          background: dark
            ? "linear-gradient(180deg, #081a3d, #0d3d85)"
            : "linear-gradient(180deg, #bcd7f8, #5d97e6)",
        }}
      >
        {BLOBS.map((b, i) => (
          <div
            key={i}
            className={cn("bw-blob")}
            style={
              {
                left: b.left,
                top: b.top,
                width: b.width,
                height: b.height,
                background: `radial-gradient(closest-side, ${b.color}, transparent 72%)`,
                "--dx": b.dx,
                "--dy": b.dy,
                "--dur": b.dur,
              } as React.CSSProperties
            }
          />
        ))}
      </div>
      {/* WebGL 流体主层 */}
      <canvas ref={glRef} className="bw-gl" />
    </div>
  );
}
