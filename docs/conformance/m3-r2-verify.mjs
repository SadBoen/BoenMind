// M3 轮 2 验证：session.search / session.fork / host 流基线帧。
// 用法：node docs/conformance/m3-r2-verify.mjs [port]
const PORT = process.argv[2] || "3079";
const API = `http://127.0.0.1:${PORT}/api`;
let failures = 0;
const ok = (name, cond, extra = "") => { console.log(cond ? "PASS" : "FAIL", name, extra); if (!cond) failures++; };

async function rpc(method, payload) {
  const r = await fetch(`${API}/${method}`, { method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ type: "client-request", rpcId: "m3r2", method, payload }) });
  return r.json();
}

// 1. 建会话 + 跑一回合（mock），产生可搜索文本
const s = await rpc("session.create", {});
const sid = s.result.value.sessionId;
ok("session.create", s.result.ok, sid);

// WS mux 驱动一回合
await new Promise((resolve, reject) => {
  const ws = new WebSocket(`ws://127.0.0.1:${PORT}/api/events.mux`);
  const t = setTimeout(() => reject(new Error("ws timeout")), 15000);
  ws.addEventListener("message", (e) => {
    const d = JSON.parse(e.data.toString());
    if (d.method === "session/event" && d.payload.event.type === "turn/end") { clearTimeout(t); ws.close(); resolve(); }
  });
  ws.addEventListener("open", async () => {
    await rpc("session.prompt", { sessionId: sid, content: [{ type: "text", text: "searchable-needle-42" }] });
  });
});

// 2. session.search 命中
const sr = await rpc("session.search", { query: "needle-42" });
ok("session.search hit", sr.result.ok && sr.result.value.items.some(i => i.sessionId === sid),
  JSON.stringify(sr.result.value.items.map(i => i.sessionId)));
ok("search snippet non-empty", sr.result.ok && sr.result.value.items.every(i => i.snippet.length > 0));

// 3. session.search 空/非法 query
const bad1 = await rpc("session.search", { query: "" });
ok("search empty query rejected", !bad1.result.ok && bad1.result.error.code === "bad-request");
const bad2 = await rpc("session.search", { query: "x".repeat(501) });
ok("search 501-char rejected", !bad2.result.ok && bad2.result.error.code === "bad-request");

// 4. session.fork（省略 atSeq → 最后完成 turn）
const fk = await rpc("session.fork", { sessionId: sid });
ok("session.fork", fk.result.ok, fk.result.value.sessionId);
const fid = fk.result.value.sessionId;
const fh = await rpc("session.history", { sessionId: fid });
const ftypes = fh.result.value.events.map(e => e.event.type);
ok("fork history has turn", ftypes.includes("turn/end"), ftypes.join(","));
ok("fork session searchable", (await rpc("session.search", { query: "needle-42" })).result.value.items.some(i => i.sessionId === fid));

// 5. session.fork 不存在的会话
const fb = await rpc("session.fork", { sessionId: "no-such-session" });
ok("fork unknown session", !fb.result.ok && fb.result.error.code === "session-not-found");

// 6. host 流基线帧：workspace-changed + session-added + session-status
// 收集窗口（400ms）而非"判据即关"——close 竞态会丢尾部帧。
function collectFrames(url, ms) {
  return new Promise((resolve, reject) => {
    const frames = [];
    const ws = new WebSocket(url);
    let timer = null;
    const stop = (ok) => { clearTimeout(timer); try { ws.close(); } catch {} resolve(ok); };
    ws.addEventListener("message", (e) => {
      frames.push(JSON.parse(e.data.toString()));
      clearTimeout(timer);
      timer = setTimeout(() => stop(frames), ms);
    });
    ws.addEventListener("error", () => reject(new Error("ws error")));
    timer = setTimeout(() => stop(frames), ms);
  });
}
const hostFrames = await collectFrames(`ws://127.0.0.1:${PORT}/api/events.host`, 400);
ok("host baseline workspace-changed", hostFrames.some(f => f.method === "host/workspace-changed"));
ok("host baseline session-added (fork)", hostFrames.some(f => f.method === "host/session-added" && f.payload.sessionId === fid));
ok("host baseline session-status", hostFrames.some(f => f.method === "host/session-status" && f.payload.running === false));
ok("host baseline frame shape", hostFrames.every(f => f.type === "server-request" && f.method && f.payload !== undefined));

// 7. workspace 变更实时广播（连接 open 后 create → 应收到 workspace-changed）
// 竞态注意：create 的广播帧可能先于 HTTP 响应到达，故 open 回调设置 wid 后回扫已收集帧。
const liveHit = await new Promise((resolve, reject) => {
  const frames = [];
  let liveWid = null;
  const ws = new WebSocket(`ws://127.0.0.1:${PORT}/api/events.host`);
  const hit = () => liveWid && frames.some(f => f.method === "host/workspace-changed"
    && f.payload.items.some(w => w.workspaceId === liveWid));
  const t = setTimeout(() => { try { ws.close(); } catch {} resolve(hit()); }, 6000);
  ws.addEventListener("message", (e) => {
    frames.push(JSON.parse(e.data.toString()));
    if (hit()) { clearTimeout(t); try { ws.close(); } catch {} resolve(true); }
  });
  ws.addEventListener("error", () => reject(new Error("host ws error")));
  ws.addEventListener("open", async () => {
    const r = await rpc("workspace.create", { path: process.cwd() });
    liveWid = r.result.value.workspace.workspaceId;
    if (hit()) { clearTimeout(t); try { ws.close(); } catch {} resolve(true); }
  });
});
ok("host live workspace-changed on create", liveHit);

// 8. prompt 触发 running 状态翻转广播
const sid2 = (await rpc("session.create", {})).result.value.sessionId;
await new Promise((resolve, reject) => {
  const ws = new WebSocket(`ws://127.0.0.1:${PORT}/api/events.host`);
  const t = setTimeout(() => reject(new Error("timeout")), 15000);
  let runningTrue = false, runningFalse = false;
  ws.addEventListener("message", (e) => {
    const d = JSON.parse(e.data.toString());
    if (d.method === "host/session-status" && d.payload.sessionId === sid2) {
      if (d.payload.running === true) runningTrue = true;
      if (d.payload.running === false && runningTrue) { runningFalse = true; clearTimeout(t); ws.close(); resolve(); }
    }
  });
  ws.addEventListener("open", async () => {
    await rpc("session.prompt", { sessionId: sid2, content: [{ type: "text", text: "status flip" }] });
  });
}).then(() => ok("host live session-status flip", true), (e) => { console.log("FAIL status flip:", e.message); failures++; });

console.log(failures === 0 ? "\n=== M3 R2 VERIFY PASS ===" : `\n=== M3 R2 VERIFY FAIL (${failures}) ===`);
process.exit(failures === 0 ? 0 : 1);
