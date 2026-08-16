/**
 * 背景特效层（2026-08-16）：独立于皮肤/壁纸的动画层，叠加在壁纸之上。
 * - EffectWave：整屏蓝色流体波浪（WebGL2 域扭曲 fbm + sin 波浪），30fps 动画。
 *
 * 实现说明（git 证据链 + 参考项目对照，最终定版）：
 * 参考项目 @deepseek-ai/dsh-client-ui-aqua（DSH-Transparent-UI-Plugin）的
 * "Living fluid board" = 整屏 WebGL 流体 shader + rAF 动画（毛玻璃面板后的
 * 背景）——本项目当初的 FluidWave/EffectWave shader 即由其移植。
 * git 证据：ba2d1df 时代同机制实现曾像素级验证有效（"实测 1.2s 内 73.6%
 * 像素变化"）；7dcf783 改为透明 canvas + mix-blend-overlay 后用户确认不动
 * （Chromium 对 blend-mode 元素有不重绘的已知问题 issue 503638）。
 * → 定版 = 参考项目同款组合：WebGL2 不透明（alpha:false）+ rAF 30fps +
 *   u_time 驱动 + 无混合模式。
 *
 * ⚠️ 注意：ZCode IAB（内嵌 webview）里 WebGL drawing buffer 不呈现（纯红
 * shader 测试实证）——IAB 内测不到画面属环境限制，真实浏览器/桌面版正常。
 *
 * - reduce-motion 开启时只渲染一帧静态纹理并停止调度（无障碍契约）；
 * - 后台标签页/窗口隐藏时暂停循环（还原时补一帧）；
 * - dataset.frames/lastTime/shaderOk 帧心跳（交接文档实证有效的观测手段）。
 */
import { useEffect, useRef } from "react";
import { useTheme } from "next-themes";
import { useAppStore } from "@/stores/app-store";

/** 亮/暗两套配色（蓝白波浪；暗色用深蓝底 + 亮蓝浪） */
const PALETTES = {
  light: { bg: [0.42, 0.55, 0.84], wave: [0.96, 0.98, 1.0] },
  dark: { bg: [0.1, 0.22, 0.5], wave: [0.72, 0.85, 1.0] },
} as const;

const VERTEX = `#version 300 es
in vec2 a_pos;
out vec2 vUv;
void main() {
  vUv = a_pos * 0.5 + 0.5;
  gl_Position = vec4(a_pos, 0.0, 1.0);
}
`;

/** 自研流体：值噪声 fbm 双层域扭曲 + sin 波浪 + 蓝白混合（u_time 驱动缓慢流动） */
const FRAGMENT = `#version 300 es
precision mediump float;
in vec2 vUv;
out vec4 fragColor;
uniform vec2 u_resolution;
uniform float u_time;
uniform vec3 u_bg;
uniform vec3 u_wave;

float hash(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}
float vnoise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
    mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x),
    u.y
  );
}
float fbm(vec2 p) {
  float v = 0.0;
  float a = 0.5;
  for (int i = 0; i < 5; i++) {
    v += a * vnoise(p);
    p = p * 2.03 + vec2(11.7, 5.3);
    a *= 0.5;
  }
  return v;
}

void main() {
  vec2 uv = vUv;
  float aspect = u_resolution.x / u_resolution.y;
  float t = u_time;

  // 双层域扭曲：q 驱动 r，r 驱动最终噪声（经典 fluid 风格）；t 驱动缓慢流动
  vec2 base = uv * 2.2 * vec2(aspect, 1.0);
  vec2 q = vec2(fbm(base + t * 0.32), fbm(base + vec2(7.3, 2.1) - t * 0.26));
  vec2 r = vec2(fbm(base + 1.6 * q + t * 0.18), fbm(base + 1.6 * q + vec2(3.9, 8.7) - t * 0.14));
  float f = fbm(base * 1.18 + 2.4 * r);

  // 波浪条纹：sin 主波 + 噪声调制 + 时间相位，形成流动浪带
  float wave = 0.5 + 0.5 * sin((uv.x * 9.0 + f * 7.0 + t * 1.1) * 3.14159);
  float mixer = clamp(f * 0.75 + wave * 0.45, 0.0, 1.0);

  // 蓝→白 soft blend（波浪亮、底色深）
  vec3 col = mix(u_bg, u_wave, smoothstep(0.25, 0.85, mixer));
  fragColor = vec4(col, 1.0);
}
`;

function compile(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader | null {
  const sh = gl.createShader(type);
  if (!sh) return null;
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
    console.error("effect wave shader:", gl.getShaderInfoLog(sh));
    return null;
  }
  return sh;
}

/**
 * 波浪特效程序缓存：shader 编译只在首次做一次——每帧重编译会卡死整页
 * （7dcf783 已修复的坑）。canvas → { gl, prog, buf, loc } 惰性初始化。
 */
const waveProgCache = new WeakMap<HTMLCanvasElement, { gl: WebGL2RenderingContext; prog: WebGLProgram; buf: WebGLBuffer; loc: number } | null>();

function getWaveProgram(canvas: HTMLCanvasElement) {
  const cached = waveProgCache.get(canvas);
  if (cached !== undefined) return cached;
  // alpha:false 不透明——参考项目（aqua fluid）与 ba2d1df 验证有效的配置
  const gl = canvas.getContext("webgl2", { alpha: false, antialias: false });
  if (!gl) {
    canvas.dataset.shaderOk = "0";
    waveProgCache.set(canvas, null);
    return null;
  }
  const vs = compile(gl, gl.VERTEX_SHADER, VERTEX);
  const fs = compile(gl, gl.FRAGMENT_SHADER, FRAGMENT);
  if (!vs || !fs) {
    canvas.dataset.shaderOk = "0";
    waveProgCache.set(canvas, null);
    return null;
  }
  const prog = gl.createProgram();
  if (!prog) {
    canvas.dataset.shaderOk = "0";
    waveProgCache.set(canvas, null);
    return null;
  }
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
    canvas.dataset.shaderOk = "0";
    waveProgCache.set(canvas, null);
    return null;
  }
  const buf = gl.createBuffer();
  if (!buf) {
    canvas.dataset.shaderOk = "0";
    waveProgCache.set(canvas, null);
    return null;
  }
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(prog, "a_pos");
  const entry = { gl, prog, buf, loc };
  canvas.dataset.shaderOk = "1";
  waveProgCache.set(canvas, entry);
  return entry;
}

/** 渲染一帧流体到 canvas（time 秒） */
function renderWave(canvas: HTMLCanvasElement, dark: boolean, time: number) {
  const entry = getWaveProgram(canvas);
  if (!entry) return;
  const { gl, prog, buf, loc } = entry;
  gl.useProgram(prog);
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);

  const pal = dark ? PALETTES.dark : PALETTES.light;
  gl.uniform2f(gl.getUniformLocation(prog, "u_resolution"), canvas.width, canvas.height);
  gl.uniform1f(gl.getUniformLocation(prog, "u_time"), time);
  gl.uniform3fv(gl.getUniformLocation(prog, "u_bg"), new Float32Array(pal.bg));
  gl.uniform3fv(gl.getUniformLocation(prog, "u_wave"), new Float32Array(pal.wave));

  gl.viewport(0, 0, canvas.width, canvas.height);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
}

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
      renderWave(canvas, resolvedTheme === "dark", timeSec);
      // 帧心跳观测（交接文档实证有效的排查手段）
      canvas.dataset.frames = String(Number(canvas.dataset.frames ?? 0) + 1);
      canvas.dataset.lastTime = String(Math.round(performance.now()));
    };

    const loop = (t: number) => {
      raf = requestAnimationFrame(loop);
      // 30fps 节流（Aqua 同款：低开销动画，避免空转 GPU）
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
