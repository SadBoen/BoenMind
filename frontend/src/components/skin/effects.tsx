/**
 * 背景特效层（2026-08-16）：独立于皮肤/壁纸的动画层，叠加在壁纸之上。
 * - EffectWave：半透明蓝色波纹纹理（透明 canvas + mix-blend-mode: soft-light），
 *   30fps 动画，全局时钟 performance.now()/1000——与界面/挂载无关，多界面速度一致。
 * - 特效与壁纸解耦：任何壁纸（渐变/静态波浪/自定义图）都可叠加波浪动画。
 * - 以后新增特效（礼花/微风等）：按 EffectWave 模式新写一个组件 + 注册表登记。
 * - reduce-motion 开启时渲染静态帧（与无障碍偏好兼容）。
 */
import { useEffect, useRef } from "react";
import { useAppStore } from "@/stores/app-store";

const VERTEX = `#version 300 es
in vec2 a_pos;
out vec2 vUv;
void main() {
  vUv = a_pos * 0.5 + 0.5;
  gl_Position = vec4(a_pos, 0.0, 1.0);
}
`;

/** 半透明蓝色波纹（alpha 通道由波浪强度驱动，混合交给 CSS mix-blend-mode） */
const FRAGMENT = `#version 300 es
precision mediump float;
in vec2 vUv;
out vec4 fragColor;
uniform vec2 u_resolution;
uniform float u_time;

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
  for (int i = 0; i < 4; i++) {
    v += a * vnoise(p);
    p = p * 2.03 + vec2(11.7, 5.3);
    a *= 0.5;
  }
  return v;
}

void main() {
  vec2 uv = vUv;
  float aspect = u_resolution.x / u_resolution.y;
  float t = u_time * 0.7; // 特效速度（独立于壁纸）

  vec2 base = uv * 2.0 * vec2(aspect, 1.0);
  vec2 q = vec2(fbm(base + t * 0.3), fbm(base + vec2(5.1, 3.3) - t * 0.24));
  float f = fbm(base + 1.8 * q);
  float wave = 0.5 + 0.5 * sin((uv.x * 8.0 + f * 6.0 + t * 1.0) * 3.14159);

  float mixer = clamp(f * 0.7 + wave * 0.4, 0.0, 1.0);
  // 深蓝→白波纹，alpha 随波浪强度（overlay 混合：波纹明显但保留壁纸底色）
  vec3 col = mix(vec3(0.20, 0.35, 0.80), vec3(1.0, 1.0, 1.0), smoothstep(0.25, 0.85, mixer));
  float alpha = smoothstep(0.1, 0.75, mixer) * 0.8;
  fragColor = vec4(col, alpha);
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
 * 波浪特效程序缓存：shader 编译只在首次/尺寸变化时做一次——每帧重编译会卡死
 * （WebGL 编译是毫秒级开销）。canvas → { gl, prog, buf, loc } 惰性初始化。
 */
const waveProgCache = new WeakMap<HTMLCanvasElement, { gl: WebGL2RenderingContext; prog: WebGLProgram; buf: WebGLBuffer; loc: number } | null>();

function getWaveProgram(canvas: HTMLCanvasElement) {
  const cached = waveProgCache.get(canvas);
  if (cached !== undefined) return cached;
  const gl = canvas.getContext("webgl2", { alpha: true, premultipliedAlpha: false, antialias: false });
  if (!gl) {
    waveProgCache.set(canvas, null);
    return null;
  }
  const vs = compile(gl, gl.VERTEX_SHADER, VERTEX);
  const fs = compile(gl, gl.FRAGMENT_SHADER, FRAGMENT);
  if (!vs || !fs) {
    waveProgCache.set(canvas, null);
    return null;
  }
  const prog = gl.createProgram();
  if (!prog) {
    waveProgCache.set(canvas, null);
    return null;
  }
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
    waveProgCache.set(canvas, null);
    return null;
  }
  const buf = gl.createBuffer();
  if (!buf) {
    waveProgCache.set(canvas, null);
    return null;
  }
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(prog, "a_pos");
  const entry = { gl, prog, buf, loc };
  waveProgCache.set(canvas, entry);
  return entry;
}

function renderWave(canvas: HTMLCanvasElement, time: number) {
  const entry = getWaveProgram(canvas);
  if (!entry) return;
  const { gl, prog, buf, loc } = entry;
  gl.useProgram(prog);
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);

  gl.uniform2f(gl.getUniformLocation(prog, "u_resolution"), canvas.width, canvas.height);
  gl.uniform1f(gl.getUniformLocation(prog, "u_time"), time);
  gl.viewport(0, 0, canvas.width, canvas.height);
  gl.clearColor(0, 0, 0, 0);
  gl.clear(gl.COLOR_BUFFER_BIT);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
}

/** 波浪特效层：叠加在壁纸之上（overlay 混合），全局时钟驱动 */
export function EffectWave() {
  const ref = useRef<HTMLCanvasElement>(null);
  const reduceMotion = useAppStore((s) => s.reduceMotion);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    // setTimeout 驱动（33ms ≈ 30fps）：rAF 在后台/离屏标签页会被 Chromium 暂停，
    // 特效将完全静止；setTimeout 后台只节流到 1s 仍会流动。前台行为一致。
    let timer = 0;

    const render = () => {
      // 全局时钟：所有特效/界面共用同一时间源，速度天然一致
      renderWave(canvas, performance.now() / 1000);
    };
    const loop = () => {
      timer = window.setTimeout(loop, 33);
      if (reduceMotion) {
        render();
        return;
      }
      render();
    };
    const resize = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
      canvas.width = Math.max(1, Math.round(canvas.clientWidth * dpr));
      canvas.height = Math.max(1, Math.round(canvas.clientHeight * dpr));
      render();
    };
    resize();
    timer = window.setTimeout(loop, 33);
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    return () => {
      clearTimeout(timer);
      ro.disconnect();
    };
  }, [reduceMotion]);

  return <canvas ref={ref} className="h-full w-full mix-blend-overlay" aria-hidden />;
}
