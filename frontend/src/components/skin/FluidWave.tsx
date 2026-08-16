/**
 * 蓝色波浪壁纸渲染（2026-08-16）：自研静态流体波浪（WebGL2 单帧渲染）。
 *
 * 算法家族参考 dsh-client-ui-aqua 的 fluid shader（其移植自 deepseek.com 官网
 * join-section 流体背景）：域扭曲 fbm 噪声 + 波浪条纹 + 蓝白多色 soft blend。
 * 实现为自研（值噪声 + 双层域扭曲 + sin 波浪混合），且刻意**只渲染静态一帧**
 * （无流场模拟、无指针交互、无动画循环）——观感等同流体首帧，零 GPU 持续开销，
 * 与 reduce-motion 兼容；后续如需动画再演进。
 *
 * 复用：全屏背景（FluidWave 组件）与设置页缩略图（mini canvas）共用 renderFluid。
 */
import { useEffect, useRef } from "react";
import { useTheme } from "next-themes";

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

/** 自研静态流体：值噪声 fbm 双层域扭曲 + sin 波浪 + 蓝白混合 */
const FRAGMENT = `#version 300 es
precision mediump float;
in vec2 vUv;
out vec4 fragColor;
uniform vec2 u_resolution;
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

  // 双层域扭曲：q 驱动 r，r 驱动最终噪声（经典 fluid 风格）
  vec2 q = vec2(fbm(uv * 2.2 * vec2(aspect, 1.0)), fbm(uv * 2.2 * vec2(aspect, 1.0) + vec2(7.3, 2.1)));
  vec2 r = vec2(fbm(uv * 2.2 * vec2(aspect, 1.0) + 1.6 * q), fbm(uv * 2.2 * vec2(aspect, 1.0) + 1.6 * q + vec2(3.9, 8.7)));
  float f = fbm(uv * 2.6 * vec2(aspect, 1.0) + 2.4 * r);

  // 波浪条纹：sin 主波 + 噪声调制，形成流体浪带
  float wave = 0.5 + 0.5 * sin((uv.x * 9.0 + f * 7.0) * 3.14159);
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

/** 渲染一帧静态流体到 canvas（全屏背景与设置页缩略图共用） */
export function renderFluid(canvas: HTMLCanvasElement, dark: boolean) {
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
  gl.uniform3fv(gl.getUniformLocation(prog, "u_bg"), new Float32Array(pal.bg));
  gl.uniform3fv(gl.getUniformLocation(prog, "u_wave"), new Float32Array(pal.wave));

  gl.viewport(0, 0, canvas.width, canvas.height);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
}

/** 全屏流体背景组件（静态一帧；随明暗主题重绘） */
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
      renderFluid(canvas, resolvedTheme === "dark");
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    return () => ro.disconnect();
  }, [resolvedTheme]);

  return <canvas ref={ref} className="h-full w-full" aria-hidden />;
}
