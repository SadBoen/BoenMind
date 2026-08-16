/**
 * 蓝色波浪壁纸渲染（2026-08-16）：自研流体波浪（WebGL2 域扭曲 fbm + sin 波浪）。
 *
 * 算法家族参考 dsh-client-ui-aqua 的 fluid shader（其移植自 deepseek.com 官网
 * join-section 流体背景）：域扭曲 fbm 噪声 + 波浪条纹 + 蓝白多色 soft blend。
 * 实现为自研（值噪声 + 双层域扭曲 + sin 波浪混合）。
 *
 * 动画：30fps 节流（Aqua 同款）+ dpr 上限 1.5；用户开"减少动画"时渲染静态帧
 * （与 reduce-motion 兼容）。背景动画时面板 backdrop-blur 每帧重算，30fps 下
 * 现代 GPU 可承受；卡顿可再降 dpr。
 *
 * 复用：全屏背景（FluidWave 组件）与设置页缩略图（mini canvas）共用 renderFluid。
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
    console.error("fluid wave shader:", gl.getShaderInfoLog(sh));
    return null;
  }
  return sh;
}

/** 渲染一帧流体到 canvas（全屏背景与设置页缩略图共用；time 秒，缩略图传 0 静态） */
export function renderFluid(canvas: HTMLCanvasElement, dark: boolean, time = 0) {
  const gl = canvas.getContext("webgl2", { alpha: false, antialias: false });
  if (!gl) return;

  const vs = compile(gl, gl.VERTEX_SHADER, VERTEX);
  const fs = compile(gl, gl.FRAGMENT_SHADER, FRAGMENT);
  if (!vs || !fs) return;
  const prog = gl.createProgram();
  if (!prog) return;
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return;
  gl.useProgram(prog);

  // 全屏三角形
  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(prog, "a_pos");
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

/** 全屏流体背景组件：30fps 动画（减少动画开启时渲染静态帧；随明暗主题重绘） */
export function FluidWave() {
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
    };

    const loop = (t: number) => {
      raf = requestAnimationFrame(loop);
      if (reduceMotion) {
        // 减少动画：只画一帧（t=0 静态图），不再进 30fps 循环
        render(0);
        return;
      }
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
    raf = requestAnimationFrame(loop);
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
  }, [resolvedTheme, reduceMotion]);

  return <canvas ref={ref} className="h-full w-full" aria-hidden />;
}
