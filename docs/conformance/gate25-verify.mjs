// 门禁 2.5 等价验证：模拟 dsh 浏览器前端完整调用序列。
// 通过标准：每步 ok:true，WS 帧 seq 连续、带 sessionId，history 含完整回合。


const API = "http://127.0.0.1:3079/api";
let failures = 0;
async function rpc(method, payload) {
  const r = await fetch(`${API}/${method}`, { method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ type: "client-request", rpcId: "gate25", method, payload }) });
  const d = await r.json();
  if (!d.result.ok) { console.log("FAIL", method, JSON.stringify(d.result.error)); failures++; }
  return d.result;
}
const ok = (name, cond, extra = "") => { console.log(cond ? "PASS" : "FAIL", name, extra); if (!cond) failures++; };

// 1. 启动链（浏览器页面加载时调用）
await rpc("host.describe", {});
await rpc("session.list", {});
await rpc("workspace.list", {});
await rpc("agentPreset.list", {});
await rpc("llm.providers", {});
await rpc("llm.models", {});
await rpc("settings.describe", {});
await rpc("credentials.describe", { refs: [] });
await rpc("skill.list", { sessionId: "probe" });
ok("startup RPC chain", true);

// 2. 内测声明 ack（settings.mutate）
const ack = await rpc("settings.mutate", { ns: "ui-onboarding", ops: [{ op: "set", path: ["welcomeNoticeVersion"], value: "2026-08-13.1" }] });
ok("settings.mutate onboarding", ack.ok, JSON.stringify(ack.value.value));

// 3. 工作区：pickDirectory（默认 cwd）→ create
const pick = await rpc("host.pickDirectory", {});
ok("host.pickDirectory", pick.ok && typeof pick.value.path === "string", pick.value.path);
const w = await rpc("workspace.create", { path: pick.value.path });
ok("workspace.create (idempotent)", w.ok && typeof w.value.created === "boolean", w.value.workspace.title);
const wid = w.value.workspace.workspaceId;

// 4. session.create(workspaceId) → 会话挂进 workspace.sessionIds
const s = await rpc("session.create", { workspaceId: wid });
const sid = s.value.sessionId;
ok("session.create", s.ok);
const wl = await rpc("workspace.list", {});
ok("workspace attach", wl.value.items[0].sessionIds.includes(sid), JSON.stringify(wl.value.items[0].sessionIds));

// 5. session.models（composer 使能）+ selectModel
const m = await rpc("session.models", { sessionId: sid });
ok("session.models", m.ok && m.value.routable === true);
const sm = await rpc("session.selectModel", { sessionId: sid, provider: "mock", model: "mock-1" });
ok("session.selectModel", sm.ok, JSON.stringify(sm.value.selected));

// 6. WS mux：连接后发 prompt，验证帧 seq 连续 + sessionId 正确
const frames = [];
const wsDone = new Promise((resolve, reject) => {
  const ws = new WebSocket("ws://127.0.0.1:3079/api/events.mux");
  const t = setTimeout(() => reject(new Error("ws timeout")), 15000);
  ws.addEventListener("message", (e) => { const d = e.data;
    frames.push(JSON.parse(d.toString()));
    const ev = frames.filter(f => f.method === "session/event");
    if (ev.some(f => f.payload.event.type === "turn/end")) { clearTimeout(t); ws.close(); resolve(); }
  });
  ws.addEventListener("error", () => reject(new Error("ws error")));
  ws.addEventListener("open", async () => {
    await rpc("session.prompt", { sessionId: sid, content: [{ type: "text", text: "gate25" }] });
  });
});
try { await wsDone; } catch (e) { console.log("FAIL ws frames:", e.message); failures++; }
const events = frames.filter(f => f.method === "session/event");
const seqs = events.map(f => f.payload.event.seq);
// M3 对齐 DSH 流协议：mock 空脚本产出 Finish 空回合。结构性断言（与 mock 脚本内容无关）：
// seq 连续、首条 user/message、末条 turn/end、全部带 sessionId 与 data。
ok("ws frames >= 4", events.length >= 4, `got ${events.length}`);
ok("seq continuous from 0", seqs.every((s, i) => s === i), seqs.join(","));
ok("sessionId in frames", events.every(f => f.payload.sessionId === sid));
ok("frame payload shape", events.every(f => f.payload.event.type && f.payload.event.data !== undefined));
ok("first event user/message", events[0].payload.event.type === "user/message", events[0]?.payload.event.type);
ok("last event turn/end", events[events.length - 1].payload.event.type === "turn/end", events[events.length - 1]?.payload.event.type);

// 7. session.history：完整回合（user → ... → turn/end）
const h = await rpc("session.history", { sessionId: sid });
const types = h.value.events.map(e => e.event.type);
ok("history complete turn", types[0] === "user/message" && types.includes("step/start") && types[types.length - 1] === "turn/end", types.join(","));

// 8. session.rename + cancel
const rn = await rpc("session.rename", { sessionId: sid, title: "gate25 session" });
ok("session.rename", rn.ok, rn.value.title);
const cc = await rpc("session.cancel", { sessionId: sid });
ok("session.cancel", cc.ok);

// 9. host 目录面
const ld = await rpc("host.listDirectory", { path: "D:/96_CoderWorld/BoenMind/kernel" });
ok("host.listDirectory", ld.ok && Array.isArray(ld.value.entries) && ld.value.entries.length > 0, `${ld.value.entries.length} entries`);

console.log(failures === 0 ? "\n=== GATE 2.5 PASS ===" : `\n=== GATE 2.5 FAIL (${failures}) ===`);
process.exit(failures === 0 ? 0 : 1);
