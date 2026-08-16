/**
 * 自研 WebGL2 礼花背景（2026-08-16）：夜空渐变 + 稀疏闪烁星 + 错峰自动
 * 绽放的礼花（哈希驱动的确定性调度，无物理缓冲，单遍全屏着色器）。
 *
 * 机制与工程要点同 fluid-gl.ts（波光特效）：
 * - 单遍 fragment shader：6 朵自动礼花按种子错峰循环绽放（约 7s 一轮），
 *   每朵 28 粒子（径向速度 + 重力回落 + 渐隐）+ 白芯 + 拖尾 + 爆心闪光；
 * - 点击任意处立即在该点绽放一朵（window pointerdown，节流 0.6s）——
 *   与波光"指针扰动"对应的直观交互（"活着"的证明）；
 * - rAF 冻结看门狗契约：帧心跳写 canvas.dataset.frames/lastTime，由宿主
 *   组件监测冻结并切换 CSS 兜底层（夜空 + 星闪 + 脉冲环）；
 * - 30fps 节流 + dpr 上限 1.25 + powerPreference:low-power + 远端粒子
 *   提前分支（d2 阈值跳过 exp，低端 GPU 友好）；
 * - reduceMotion 只渲染一帧静态夜空；WebGL 不可用返回 null。
 */

/** 观感参数（色调跟随壁纸预设/皮肤 hue，换壁纸只切 uniform 不重建 GL） */
export interface FireworksParams {
  /** 夜空底色（上） */
  skyA: [number, number, number];
  /** 夜空底色（下，地平线侧稍亮） */
  skyB: [number, number, number];
  /** 礼花主色（壁纸色调） */
  colMain: [number, number, number];
  /** 礼花补色（主色 +40°） */
  colAccent: [number, number, number];
}

/** hsl → rgb（0-1）；与 fluid-gl.ts 同款（独立复制，自包含零耦合） */
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
 * 按色调生成礼花观感：礼花需要夜空底（明暗主题都用深色夜，
 * 亮主题地平线侧略亮，保证玻璃面板可读性）——特效自带完整画面盖过壁纸。
 */
export function fireworksFromHue(hue: number, dark: boolean): FireworksParams {
  return {
    skyA: dark ? hslToRgb(hue, 0.42, 0.075) : hslToRgb(hue, 0.45, 0.15),
    skyB: dark ? hslToRgb(hue, 0.5, 0.17) : hslToRgb(hue, 0.5, 0.26),
    colMain: dark ? hslToRgb(hue, 0.85, 0.66) : hslToRgb(hue, 0.9, 0.62),
    colAccent: hslToRgb(hue + 40, 0.9, 0.62),
  };
}

const VERT = `#version 300 es
in vec2 aPos;
out vec2 vUv;
void main() { vUv = aPos * .5 + .5; gl_Position = vec4(aPos, 0., 1.); }
`;

const FRAG = `#version 300 es
precision highp float;
uniform float uTime;
uniform vec3 uSkyA;
uniform vec3 uSkyB;
uniform vec3 uColMain;
uniform vec3 uColAccent;
uniform vec3 uClick; // xy=点击 uv，z=绽放时刻（0 = 尚无点击）
in vec2 vUv;
out vec4 outColor;

float hash1(float n) { return fract(sin(n * 127.1) * 43758.5453123); }
float hash1b(float n) { return fract(sin(n * 311.7) * 12345.6789); }

/** 一朵礼花：径向粒子 + 白芯 + 拖尾 + 爆心闪光；未到/已谢返回 0 */
vec3 burst(vec2 uv, vec2 origin, float age, float life, float seed, vec3 colMain, vec3 colAccent) {
  if (age < 0.0 || age > life) return vec3(0.0);
  float t01 = age / life;
  float fade = (1.0 - t01) * (1.0 - t01);
  vec3 acc = vec3(0.0);
  for (int j = 0; j < 28; j++) {
    float fj = float(j);
    float ang = hash1(fj * 17.13 + seed) * 6.2831853;
    float speed = 0.16 + 0.24 * hash1b(fj * 29.9 + seed * 3.7);
    vec2 dir = vec2(cos(ang), sin(ang) * 0.92);
    // 抛物线：径向初速 + 重力回落（uv y 向上）
    vec2 pos = origin + dir * speed * age - vec2(0.0, 0.34) * age * age;
    float d2 = dot(uv - pos, uv - pos);
    if (d2 < 0.09) { // 远端粒子提前退出（低端 GPU 友好）
      vec3 col = mix(colMain, colAccent, hash1(fj * 3.3 + seed));
      acc += col * exp(-d2 * 260.0) * fade;
      acc += vec3(1.0) * exp(-d2 * 1400.0) * fade * 0.9;            // 白芯
      vec2 tail = pos - dir * speed * 0.05 * (1.0 - t01 * 0.6);
      acc += col * exp(-dot(uv - tail, uv - tail) * 620.0) * fade * 0.45; // 拖尾
    }
  }
  // 爆心闪光（绽放瞬间最亮）
  acc += vec3(1.0) * exp(-age * 16.0) * exp(-dot(uv - origin, uv - origin) * 900.0) * 0.6;
  return acc;
}

void main() {
  // 夜空（上深下浅）
  vec3 col = mix(uSkyA, uSkyB, smoothstep(0.05, 1.0, vUv.y));
  // 稀疏星星（网格哈希 + 独立相位闪烁）
  vec2 cell = floor(vUv * vec2(64.0, 40.0));
  float h = hash1(cell.x * 91.7 + cell.y * 13.1);
  if (h > 0.92) {
    float tw = 0.5 + 0.5 * sin(uTime * (1.5 + h * 3.0) + h * 40.0);
    col += vec3(0.8, 0.9, 1.0) * smoothstep(0.92, 1.0, h) * tw * 0.5;
  }
  // 自动礼花：6 朵错峰循环（约 7.5s 一轮，任意时刻约 2 朵可见）
  for (int i = 0; i < 6; i++) {
    float fi = float(i);
    float seed = fi * 7.31;
    float spawn = fi * 1.25 + hash1(seed) * 0.9;
    float life = 2.7 + hash1b(seed) * 0.5;
    float age = uTime - spawn;
    vec2 origin = vec2(0.14 + 0.72 * hash1(seed * 1.7), 0.42 + 0.42 * hash1b(seed * 2.3));
    col += burst(vUv, origin, age, life, seed, uColMain, uColAccent);
  }
  // 点击礼花（节流由宿主/监听处控制）
  if (uClick.z > 0.0) {
    col += burst(vUv, uClick.xy, uTime - uClick.z, 3.0, 999.0, uColMain, uColAccent);
  }
  outColor = vec4(col, 1.0);
}
`;

export interface FireworksHandle {
  /** 切换观感参数（主题/壁纸切换用） */
  setParams(params: FireworksParams): void;
  /** 帧心跳（渲染帧数；冻结时停走，看门狗据此切换兜底） */
  frames(): number;
  dispose(): void;
}

/**
 * 挂载 WebGL2 礼花动画。WebGL2 不可用返回 null（宿主走 CSS 兜底层）。
 * reduceMotion=true 时渲染一帧静态夜空后不再调度。
 */
export function attachFireworks(
  canvas: HTMLCanvasElement,
  params: FireworksParams,
  reduceMotion: boolean,
): FireworksHandle | null {
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
      console.error("fireworks shader:", gl.getShaderInfoLog(sh));
      return null;
    }
    return sh;
  };
  const vs = compile(gl.VERTEX_SHADER, VERT);
  const fs = compile(gl.FRAGMENT_SHADER, FRAG);
  if (!vs || !fs) return null;
  const prog = gl.createProgram();
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return null;

  const quad = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, quad);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(prog, "aPos");
  gl.bindBuffer(gl.ARRAY_BUFFER, quad);
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);

  let width = 0;
  let height = 0;
  const resize = () => {
    const dpr = Math.min(window.devicePixelRatio || 1, 1.25);
    width = Math.max(1, Math.round(canvas.clientWidth * dpr));
    height = Math.max(1, Math.round(canvas.clientHeight * dpr));
    canvas.width = width;
    canvas.height = height;
  };
  resize();

  let current = { ...params };
  const start = performance.now();
  let frames = 0;
  let raf = 0;
  let last = 0;

  const draw = (nowMs: number) => {
    gl.viewport(0, 0, width, height);
    gl.useProgram(prog);
    gl.uniform1f(gl.getUniformLocation(prog, "uTime"), (nowMs - start) / 1000);
    gl.uniform3f(gl.getUniformLocation(prog, "uSkyA"), current.skyA[0], current.skyA[1], current.skyA[2]);
    gl.uniform3f(gl.getUniformLocation(prog, "uSkyB"), current.skyB[0], current.skyB[1], current.skyB[2]);
    gl.uniform3f(gl.getUniformLocation(prog, "uColMain"), current.colMain[0], current.colMain[1], current.colMain[2]);
    gl.uniform3f(gl.getUniformLocation(prog, "uColAccent"), current.colAccent[0], current.colAccent[1], current.colAccent[2]);
    gl.uniform3f(gl.getUniformLocation(prog, "uClick"), click.x, click.y, click.t);
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

  // 点击绽放：window 级监听（背景层 pointer-events-none，UI 点击也可绽放，
  // 节流 0.6s 防连点刷屏）
  const click = { x: 0.5, y: 0.6, t: 0 };
  let lastClick = 0;
  const onClick = (e: PointerEvent) => {
    const now = performance.now();
    if (now - lastClick < 600) return;
    lastClick = now;
    click.x = e.clientX / window.innerWidth;
    click.y = 1 - e.clientY / window.innerHeight;
    click.t = (now - start) / 1000;
    draw(now); // 立即渲染一帧，点击即见
  };
  window.addEventListener("pointerdown", onClick, { passive: true });

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
      window.removeEventListener("pointerdown", onClick);
      gl.deleteBuffer(quad);
      gl.deleteProgram(prog);
    },
  };
}
