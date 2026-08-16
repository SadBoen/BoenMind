/**
 * 自研 WebGL2 流体背景（2026-08-16）：波光粼粼观感——domain-warp 噪声 + 涡旋
 * 迭代 + 三色柔混 + 指针流场扰动。
 *
 * 机制参照 deepseek 官网 join-section 流体（DSH-Transparent-UI-Plugin 同源
 * 观感），代码自研（抄机制不抄代码）：两个渲染通道——
 *   1) 扰动场（1/4 分辨率 ping-pong）：指针位置/速度写入会衰减的流场；
 *   2) 显示（全分辨率）：噪声角度扭转坐标 + 流场推移 + 涡旋迭代，三色
 *      smoothstep 柔混成流动的液面光。
 *
 * 工程要点（本会话实测教训，见 effects.tsx 架构注记）：
 * - rAF 在可见性误报环境会被整段掐死 → 帧心跳写 canvas.dataset，由宿主
 *   组件的看门狗监测冻结并切换 CSS 兜底层；
 * - 30fps 节流 + dpr 上限 1.5 + powerPreference:low-power（官网同策略）；
 * - reduceMotion 只渲染一帧静态画面；
 * - WebGL 不可用时返回 null，宿主走 2D 静态帧兜底。
 */

/** 观感参数（可在调参时直接改这里） */
export interface FluidParams {
  /** 噪声尺度（屏宽噪声单元数） */
  scale: number;
  /** 域扭曲总量 */
  flow: number;
  /** 涡旋强度 */
  swirl: number;
  /** 涡旋迭代次数 */
  swirlIters: number;
  /** 时间倍率（流速） */
  speed: number;
  /** 扰动衰减（每帧） */
  decay: number;
  /** 三色（底色 → 亮色 → 高光） */
  colA: [number, number, number];
  colB: [number, number, number];
  colC: [number, number, number];
}

/** hsl → rgb（0-1） */
function hslToRgb(hDeg: number, s: number, l: number): [number, number, number] {
  const h = ((hDeg % 360) + 360) % 360 / 360;
  if (s === 0) return [l, l, l];
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const conv = (t: number) => {
    let tt = t;
    if (tt < 0) tt += 1;
    if (tt > 1) tt -= 1;
    if (tt < 1 / 6) return p + (q - p) * 6 * tt;
    if (tt < 1 / 2) return q;
    if (tt < 2 / 3) return p + (q - p) * (2 / 3 - tt) * 6;
    return p;
  };
  return [conv(h + 1 / 3), conv(h), conv(h - 1 / 3)];
}

/**
 * 按色调生成流体观感（波光颜色跟随壁纸预设/皮肤色调）：
 * 暗色 = 深底 + 中亮同色 + 淡色高光；亮色 = 中底 + 淡同色 + 白高光。
 * 强度刻意压淡（2026-08-16 用户反馈"太浓"）：混色阈值收窄 + 中亮色降明度。
 */
export function fluidFromHue(hue: number, dark: boolean): FluidParams {
  return {
    scale: 8,
    flow: 0.7,
    swirl: 0.22,
    swirlIters: 6,
    speed: 0.16,
    decay: 0.96,
    colA: dark ? hslToRgb(hue, 0.58, 0.15) : hslToRgb(hue, 0.38, 0.74),
    colB: dark ? hslToRgb(hue, 0.5, 0.44) : hslToRgb(hue, 0.3, 0.9),
    colC: dark ? hslToRgb(hue, 0.25, 0.78) : [1, 1, 1],
  };
}

const VERT = `#version 300 es
in vec2 aPos;
out vec2 vUv;
void main() { vUv = aPos * .5 + .5; gl_Position = vec4(aPos, 0., 1.); }
`;

/** 扰动场：r=强度，gb=方向（.5 为静止），指针画刷写入，逐帧衰减 */
const STIR_SHADER = `#version 300 es
precision mediump float;
uniform sampler2D uPrev;
uniform vec2 uPointer;
uniform vec2 uVel;
in vec2 vUv;
out vec4 outColor;
void main() {
  vec4 prev = texture(uPrev, vUv);
  prev.r *= 0.96;
  prev.gb = mix(vec2(.5), prev.gb, 0.96);
  float d = distance(vUv, uPointer);
  float inf = exp(-d * d / 0.024);
  float strength = (0.22 + min(length(uVel) * 2.5, 0.8)) * inf;
  prev.r = max(prev.r, strength);
  prev.gb = mix(prev.gb, clamp(uVel * 2. + .5, 0., 1.), inf * 0.35);
  outColor = prev;
}
`;

/** 显示：噪声角扭转 + 流场推移 + 涡旋迭代 + 三色柔混 */
const DISPLAY_SHADER = `#version 300 es
precision highp float;
uniform float uTime;
uniform vec2 uRes;
uniform float uScale;
uniform float uFlow;
uniform float uSwirl;
uniform float uSwirlIters;
uniform vec3 uColA;
uniform vec3 uColB;
uniform vec3 uColC;
uniform sampler2D uStir;
in vec2 vUv;
out vec4 outColor;

float hash(vec2 p) {
  p = fract(p * vec2(234.34, 435.345));
  p += dot(p, p + 34.23);
  return fract(p.x * p.y);
}
float vnoise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  vec2 u = f * f * (3. - 2. * f);
  return mix(
    mix(hash(i), hash(i + vec2(1., 0.)), u.x),
    mix(hash(i + vec2(0., 1.)), hash(i + vec2(1., 1.)), u.x),
    u.y
  );
}
mat2 rot(float a) { float c = cos(a), s = sin(a); return mat2(c, -s, s, c); }

void main() {
  vec2 frag = gl_FragCoord.xy / uRes;
  float t = uTime;

  vec4 stirTex = texture(uStir, frag);
  float wake = stirTex.r;
  vec2 wakeDir = (stirTex.gb - .5) * 2.;

  vec2 p = frag - .5;
  p *= uScale;
  p = rot(0.18) * p;
  p += .5;

  float ang = vnoise(p + t * .7) * 6.2831853;
  float mag = vnoise(p * 2.1 - t * .6);
  p += (uFlow + wake * 0.9) * mag * vec2(cos(ang), sin(ang));
  p += wakeDir * wake * .12;

  for (int i = 1; i <= 8; i++) {
    if (float(i) > uSwirlIters) break;
    float fi = float(i);
    p.x += uSwirl / fi * cos(t + fi * 1.5 * p.y);
    p.y += uSwirl / fi * cos(t + fi * 1.0 * p.x);
  }

  float m = .5 + .5 * sin(p.x * 3.1) * cos(p.y * 2.7);
  float g = vnoise(p * 1.7 + t * .3);
  vec3 col = mix(uColA, uColB, smoothstep(.22, .82, m));
  col = mix(col, uColC, smoothstep(.52, .98, m * g));
  outColor = vec4(col, 1.);
}
`;

export interface FluidHandle {
  /** 切换观感参数（主题切换用） */
  setParams(params: FluidParams): void;
  /** 帧心跳（渲染帧数；冻结时停走，看门狗据此切换兜底） */
  frames(): number;
  dispose(): void;
}

/**
 * 挂载 WebGL 流体动画。WebGL2 不可用返回 null（宿主走 2D 静态帧兜底）。
 * reduceMotion=true 时渲染一帧静态画面后不再调度。
 */
export function attachFluid(
  canvas: HTMLCanvasElement,
  params: FluidParams,
  reduceMotion: boolean,
): FluidHandle | null {
  const gl = canvas.getContext("webgl2", {
    alpha: false,
    powerPreference: "low-power",
  });
  if (!gl) return null;

  const compile = (type: number, src: string) => {
    const sh = gl.createShader(type);
    if (!sh) return null;
    gl.shaderSource(sh, src);
    gl.compileShader(sh);
    if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
      console.error("fluid shader:", gl.getShaderInfoLog(sh));
      return null;
    }
    return sh;
  };
  const build = (fragSrc: string) => {
    const vs = compile(gl.VERTEX_SHADER, VERT);
    const fs = compile(gl.FRAGMENT_SHADER, fragSrc);
    if (!vs || !fs) return null;
    const prog = gl.createProgram();
    gl.attachShader(prog, vs);
    gl.attachShader(prog, fs);
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return null;
    return prog;
  };

  const stirProg = build(STIR_SHADER);
  const dispProg = build(DISPLAY_SHADER);
  if (!stirProg || !dispProg) return null;

  const quad = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, quad);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  const bindQuad = (prog: WebGLProgram) => {
    const loc = gl.getAttribLocation(prog, "aPos");
    gl.bindBuffer(gl.ARRAY_BUFFER, quad);
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
  };

  // ping-pong 扰动场纹理（1/4 分辨率）
  const makeTarget = (w: number, h: number) => {
    const tex = gl.createTexture();
    if (!tex) throw new Error("fluid: texture alloc failed");
    gl.bindTexture(gl.TEXTURE_2D, tex);
    const init = new Uint8Array(w * h * 4);
    for (let i = 0; i < w * h; i++) {
      init[i * 4 + 1] = 128; // gb=.5 静止
      init[i * 4 + 3] = 255;
    }
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, init);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    const fbo = gl.createFramebuffer();
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    return { fbo, tex };
  };

  const destroyTarget = (t: { fbo: WebGLFramebuffer; tex: WebGLTexture } | null) => {
    if (!t) return;
    gl.deleteFramebuffer(t.fbo);
    gl.deleteTexture(t.tex);
  };

  let width = 0;
  let height = 0;
  let flowW = 0;
  let flowH = 0;
  let targetA: { fbo: WebGLFramebuffer; tex: WebGLTexture } | null = null;
  let targetB: { fbo: WebGLFramebuffer; tex: WebGLTexture } | null = null;
  let flip = false;

  const resize = () => {
    const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
    width = Math.max(1, Math.round(canvas.clientWidth * dpr));
    height = Math.max(1, Math.round(canvas.clientHeight * dpr));
    canvas.width = width;
    canvas.height = height;
    const nextW = Math.max(1, Math.round(width / 4));
    const nextH = Math.max(1, Math.round(height / 4));
    if (nextW === flowW && nextH === flowH) return;
    flowW = nextW;
    flowH = nextH;
    destroyTarget(targetA);
    destroyTarget(targetB);
    targetA = makeTarget(flowW, flowH);
    targetB = makeTarget(flowW, flowH);
  };
  resize();

  // 指针：平滑跟随 + 速度估计（画刷强度来自速度）
  const pointer = { x: 0.5, y: 0.5, sx: 0.5, sy: 0.5, vx: 0, vy: 0 };
  const onMove = (e: PointerEvent) => {
    const nx = e.clientX / window.innerWidth;
    const ny = 1 - e.clientY / window.innerHeight;
    pointer.vx = pointer.vx * 0.7 + (nx - pointer.x) * 0.3;
    pointer.vy = pointer.vy * 0.7 + (ny - pointer.y) * 0.3;
    pointer.x = nx;
    pointer.y = ny;
  };
  window.addEventListener("pointermove", onMove, { passive: true });

  let current = { ...params };
  const start = performance.now();
  let frames = 0;
  let raf = 0;
  let last = 0;

  const draw = (nowMs: number) => {
    // 扰动场步进
    pointer.sx += (pointer.x - pointer.sx) * 0.1;
    pointer.sy += (pointer.y - pointer.sy) * 0.1;
    pointer.vx *= 0.92;
    pointer.vy *= 0.92;

    const read = flip ? targetA : targetB;
    const write = !flip ? targetA : targetB;
    flip = !flip;

    gl.bindFramebuffer(gl.FRAMEBUFFER, write!.fbo);
    gl.viewport(0, 0, flowW, flowH);
    gl.useProgram(stirProg);
    bindQuad(stirProg);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, read!.tex);
    gl.uniform1i(gl.getUniformLocation(stirProg, "uPrev"), 0);
    gl.uniform2f(gl.getUniformLocation(stirProg, "uPointer"), pointer.sx, pointer.sy);
    gl.uniform2f(gl.getUniformLocation(stirProg, "uVel"), pointer.vx, pointer.vy);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

    // 显示帧
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, width, height);
    gl.useProgram(dispProg);
    bindQuad(dispProg);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, write!.tex);
    gl.uniform1i(gl.getUniformLocation(dispProg, "uStir"), 0);
    gl.uniform1f(gl.getUniformLocation(dispProg, "uTime"), ((nowMs - start) / 1000) * current.speed);
    gl.uniform2f(gl.getUniformLocation(dispProg, "uRes"), width, height);
    gl.uniform1f(gl.getUniformLocation(dispProg, "uScale"), current.scale);
    gl.uniform1f(gl.getUniformLocation(dispProg, "uFlow"), current.flow);
    gl.uniform1f(gl.getUniformLocation(dispProg, "uSwirl"), current.swirl);
    gl.uniform1f(gl.getUniformLocation(dispProg, "uSwirlIters"), current.swirlIters);
    gl.uniform3f(gl.getUniformLocation(dispProg, "uColA"), current.colA[0], current.colA[1], current.colA[2]);
    gl.uniform3f(gl.getUniformLocation(dispProg, "uColB"), current.colB[0], current.colB[1], current.colB[2]);
    gl.uniform3f(gl.getUniformLocation(dispProg, "uColC"), current.colC[0], current.colC[1], current.colC[2]);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

    frames += 1;
    canvas.dataset.frames = String(frames);
    canvas.dataset.lastTime = String(Math.round(nowMs));
  };

  const loop = (t: number) => {
    raf = requestAnimationFrame(loop);
    if (t - last < 33) return; // 30fps
    last = t;
    draw(t);
  };

  if (reduceMotion) {
    draw(performance.now());
  } else {
    raf = requestAnimationFrame(loop);
  }

  const ro = new ResizeObserver(() => {
    resize();
    draw(performance.now());
  });
  ro.observe(canvas);

  return {
    setParams(next) {
      current = { ...next };
      if (reduceMotion) draw(performance.now());
    },
    frames() {
      return frames;
    },
    dispose() {
      cancelAnimationFrame(raf);
      ro.disconnect();
      window.removeEventListener("pointermove", onMove);
      destroyTarget(targetA);
      destroyTarget(targetB);
      gl.deleteBuffer(quad);
      gl.deleteProgram(stirProg);
      gl.deleteProgram(dispProg);
    },
  };
}
