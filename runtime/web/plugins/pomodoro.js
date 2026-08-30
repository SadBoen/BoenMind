/* 番茄钟插件(移植版)——原作 causebefore/dsh-pomodoro(MIT License),
 * 为 BoenMind 前端界面插槽重写:去除 React/Cordis 依赖,以原生 DOM 接入
 * window.boenmind.register;功能设计对齐原作(2026-08-30 移植):
 *   · 专注/休息两阶段循环,默认 25/5 分钟(1–240 可配,localStorage 持久化)
 *   · 专注完成自动开始休息(autoBreak,原作默认开)、休息完自动下一轮
 *     (autoFocus,原作默认关);均可关
 *   · 开始/暂停/重置/跳过;🍅 完成计数;阶段切换应用内提示
 *   · 侧栏面板 ↔ 可拖动浮窗两种形态
 * 纯外观与本地计时,不调用任何后端 API。 */
(function () {
  if (window.__pomodoroInstalled) return;
  window.__pomodoroInstalled = true;

  const STORE_KEY = "bm.plugin.pomodoro.v1";
  const FOCUS = "focus", BREAK = "break";
  const DEFAULTS = { focusMin: 25, breakMin: 5, autoBreak: true, autoFocus: false };

  function loadState() {
    try { return JSON.parse(localStorage.getItem(STORE_KEY)) || {}; }
    catch (e) { return {}; }
  }
  const st = Object.assign({
    phase: FOCUS,
    deadline: 0,        // 运行中的结束时间戳(ms);0=未运行
    remaining: 25 * 60 * 1000,
    running: false,
    completed: 0,
    floating: false,
  }, loadState());
  const cfg = Object.assign({ focusMin: 25, breakMin: 5, autoBreak: true, autoFocus: false },
    loadState().cfg || {});
  function save() {
    localStorage.setItem(STORE_KEY, JSON.stringify(Object.assign({}, st, { cfg })));
  }
  const phaseMs = () => (st.phase === FOCUS ? cfg.focusMin : cfg.breakMin) * 60 * 1000;

  /* ===== UI ===== */
  const box = document.createElement("div");
  box.style.cssText = "margin:0 2px;display:grid;gap:8px";
  box.innerHTML = [
    '<div style="display:flex;align-items:center;justify-content:space-between">',
    '  <b style="font-weight:500;color:var(--bm-alias-label-primary);font-size:13px">🍅 番茄钟</b>',
    '  <span style="display:flex;gap:2px">',
    '    <button data-act="float" title="浮窗/侧栏" style="font-size:11px;padding:2px 7px;border-radius:6px;color:var(--bm-alias-label-tertiary)">浮窗</button>',
    '    <button data-act="close" title="卸载插件" style="font-size:11px;padding:2px 7px;border-radius:6px;color:var(--bm-alias-label-tertiary)">✕</button>',
    '  </span>',
    '</div>',
    '<div style="display:grid;justify-items:center;gap:6px">',
    '  <svg width="96" height="96" viewBox="0 0 120 120" style="transform:rotate(-90deg)">',
    '    <circle cx="60" cy="60" r="52" fill="none" stroke="var(--bm-specific-tip)" stroke-width="8"/>',
    '    <circle data-ring cx="60" cy="60" r="52" fill="none" stroke="var(--bm-alias-button-info-fill)" stroke-width="8" stroke-linecap="round" style="transition:stroke-dashoffset .25s linear"/>',
    '  </svg>',
    '  <div data-time style="position:relative;top:-72px;font-size:19px;font-variant-numeric:tabular-nums;color:var(--bm-alias-label-primary)">25:00</div>',
    '  <div data-phase style="position:relative;top:-66px;font-size:11px;color:var(--bm-alias-label-tertiary)">专注</div>',
    '</div>',
    '<div style="display:flex;gap:6px;justify-content:center">',
    '  <button data-act="toggle" class="pomo-primary" style="flex:1;padding:6px 0;border-radius:8px;font-size:12px;background:var(--bm-alias-button-info-fill);color:var(--bm-alias-label-primary-foreground)">开始</button>',
    '  <button data-act="reset" style="padding:6px 10px;border-radius:8px;font-size:12px;border:1px solid var(--bm-alias-border-l2);color:var(--bm-alias-label-secondary)">重置</button>',
    '  <button data-act="skip" style="padding:6px 10px;border-radius:8px;font-size:12px;border:1px solid var(--bm-alias-border-l2);color:var(--bm-alias-label-secondary)">跳过</button>',
    '</div>',
    '<div data-count style="font-size:11px;color:var(--bm-alias-label-tertiary);text-align:center">已完成 🍅 × 0</div>',
    '<details style="font-size:12px;color:var(--bm-alias-label-secondary)">',
    '  <summary style="cursor:pointer">设置</summary>',
    '  <div style="display:grid;gap:6px;margin-top:8px">',
    '    <label style="display:flex;justify-content:space-between;align-items:center;gap:6px">专注(分钟)<input data-cfg="focusMin" type="number" min="1" max="240" style="width:64px;background:var(--bm-specific-login-input);border:1px solid var(--bm-alias-border-l1);border-radius:6px;padding:3px 6px;color:var(--bm-alias-label-primary)"></label>',
    '    <label style="display:flex;justify-content:space-between;align-items:center;gap:6px">休息(分钟)<input data-cfg="breakMin" type="number" min="1" max="240" style="width:64px;background:var(--bm-specific-login-input);border:1px solid var(--bm-alias-border-l1);border-radius:6px;padding:3px 6px;color:var(--bm-alias-label-primary)"></label>',
    '    <label style="display:flex;justify-content:space-between;align-items:center;gap:6px;cursor:pointer">专注完自动休息<input data-cfg="autoBreak" type="checkbox"></label>',
    '    <label style="display:flex;justify-content:space-between;align-items:center;gap:6px;cursor:pointer">休息完自动专注<input data-cfg="autoFocus" type="checkbox"></label>',
    '  </div>',
    '</details>',
  ].join("");

  const ring = box.querySelector("[data-ring]");
  const CIRC = 2 * Math.PI * 52;
  ring.style.strokeDasharray = String(CIRC);
  const timeEl = box.querySelector("[data-time]");
  const phaseEl = box.querySelector("[data-phase]");
  const countEl = box.querySelector("[data-count]");
  const toggleBtn = box.querySelector('[data-act="toggle"]');

  function toast(msg) {
    const t = document.createElement("div");
    t.textContent = msg;
    t.style.cssText = "position:fixed;left:50%;bottom:96px;transform:translateX(-50%);" +
      "background:var(--bm-alias-toast-bg);color:var(--bm-alias-label-primary-foreground);" +
      "padding:8px 16px;border-radius:10px;font-size:13px;z-index:60;transition:opacity .4s";
    document.body.appendChild(t);
    setTimeout(() => { t.style.opacity = "0"; }, 2200);
    setTimeout(() => t.remove(), 2700);
  }
  function fmt(ms) {
    const s = Math.max(0, Math.round(ms / 1000));
    return String(Math.floor(s / 60)).padStart(2, "0") + ":" + String(s % 60).padStart(2, "0");
  }
  function render() {
    const total = phaseMs();
    const rem = st.running ? Math.max(0, st.deadline - Date.now()) : st.remaining;
    timeEl.textContent = fmt(rem);
    phaseEl.textContent = st.phase === FOCUS ? "专注" : "休息";
    ring.style.strokeDashoffset = String(CIRC * (1 - rem / total));
    toggleBtn.textContent = st.running ? "暂停" : "开始";
    countEl.textContent = "已完成 🍅 × " + st.completed;
    box.querySelector('[data-act="float"]').textContent = st.floating ? "侧栏" : "浮窗";
  }
  function completePhase() {
    st.running = false;
    if (st.phase === FOCUS) {
      st.completed += 1;
      toast("专注完成 · 🍅 × " + st.completed);
      st.phase = BREAK;
      st.remaining = phaseMs();
      if (cfg.autoBreak) start();
    } else {
      toast("休息结束 · 准备好就开下一轮");
      st.phase = FOCUS;
      st.remaining = phaseMs();
      if (cfg.autoFocus) start();
    }
    save(); render();
  }
  function start() {
    if (st.running) return;
    st.deadline = Date.now() + st.remaining;
    st.running = true;
    save(); render();
  }
  function pause() {
    if (!st.running) return;
    st.remaining = Math.max(0, st.deadline - Date.now());
    st.running = false;
    save(); render();
  }
  function enterPhase(p) {
    st.phase = p; st.remaining = phaseMs(); st.running = false;
    save(); render();
  }

  box.addEventListener("click", (ev) => {
    const b = ev.target.closest("button[data-act]");
    if (!b) return;
    const act = b.dataset.act;
    if (act === "toggle") st.running ? pause() : (st.remaining <= 0 && (st.remaining = phaseMs()), start());
    else if (act === "reset") enterPhase(st.phase);
    else if (act === "skip") completePhase();
    else if (act === "close") { if (window.__pluginPomodoroUnload) window.__pluginPomodoroUnload(); }
    else if (act === "float") { st.floating = !st.floating; applyFloat(); save(); }
  });
  box.addEventListener("change", (ev) => {
    const el = ev.target.closest("[data-cfg]");
    if (!el) return;
    const k = el.dataset.cfg;
    if (el.type === "checkbox") cfg[k] = el.checked;
    else {
      const v = Math.min(240, Math.max(1, parseInt(el.value, 10) || 0));
      if (!v) { el.value = cfg[k]; return; }
      cfg[k] = v; el.value = v;
      if (!st.running) { st.remaining = phaseMs(); }
    }
    save(); render();
  });

  /* 浮窗模式:fixed 定位 + 标题栏拖动(移植原作"可拖动浮动面板") */
  function applyFloat() {
    if (st.floating) {
      box.style.position = "fixed";
      box.style.right = "26px"; box.style.top = "90px";
      box.style.zIndex = "50"; box.style.width = "220px";
      box.style.background = "var(--bm-alias-bg-layer-2)";
      box.style.border = "1px solid var(--bm-alias-border-l2)";
      box.style.borderRadius = "14px"; box.style.padding = "12px";
      box.style.boxShadow = "0 14px 40px rgba(0,0,0,.35)";
    } else {
      box.style.cssText = "margin:0 2px;display:grid;gap:8px";
    }
  }

  let timer = setInterval(() => {
    if (st.running && Date.now() >= st.deadline) completePhase();
    render();
  }, 500);

  const ok = window.boenmind.register({ slot: "sidebar-extra", id: "pomodoro", node: box, order: 1 });
  if (!ok) { clearInterval(timer); return; }
  applyFloat(); render();

  window.__pluginPomodoroUnload = function () {
    pause();
    clearInterval(timer);
    window.boenmind.unregister("pomodoro");
    window.__pomodoroInstalled = false;
    delete window.__pluginPomodoroUnload;
  };
})();
