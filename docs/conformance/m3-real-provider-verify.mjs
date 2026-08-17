// M3 真 provider 端到端验证：真实模型流式回复 + WS 实时事件。
// 用法：node docs/conformance/m3-real-provider-verify.mjs [port]
const PORT = process.argv[2] || "3079";
const API = `http://127.0.0.1:${PORT}/api`;
let failures = 0;
const ok = (name, cond, extra = "") => { console.log(cond ? "PASS" : "FAIL", name, extra); if (!cond) failures++; };

async function rpc(method, payload) {
  const r = await fetch(`${API}/${method}`, { method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ type: "client-request", rpcId: "m3v", method, payload }) });
  return r.json();
}

// 1. 真 provider 已装配（非 mock）
const pv = await rpc("llm.providers", {});
const providers = pv.result.value.providers;
ok("llm.providers has real providers", providers.some(p => p.provider === "minimax") && providers.some(p => p.provider === "deepseek"),
  providers.map(p => p.provider).join(","));

const mm = await rpc("llm.models", {});
const groups = mm.result.value.groups;
ok("llm.models groups", groups.some(g => g.provider === "minimax"));
const minimaxModels = groups.find(g => g.provider === "minimax").models;
ok("minimax models include M3", minimaxModels.some(m => m.id === "MiniMax-M3"));

// 2. 建会话 + selectModel(minimax/MiniMax-M3) + session.models 回显
const wsRes = await rpc("workspace.create", { path: process.cwd() });
const wid = wsRes.result.value.workspace.workspaceId;
const s = await rpc("session.create", { workspaceId: wid });
const sid = s.result.value.sessionId;
ok("session.create", s.result.ok, sid);
const sel = await rpc("session.selectModel", { sessionId: sid, provider: "minimax", model: "MiniMax-M3" });
ok("session.selectModel", sel.result.ok, JSON.stringify(sel.result.value.selected));
const sm = await rpc("session.models", { sessionId: sid });
ok("session.models echoes selection", sm.result.value.current.provider === "minimax" && sm.result.value.current.model === "MiniMax-M3",
  JSON.stringify(sm.result.value.current));

// 3. WS mux 开流 → prompt 真实模型 → 收 assistant 文本
const frames = [];
const wsDone = new Promise((resolve, reject) => {
  const ws = new WebSocket(`ws://127.0.0.1:${PORT}/api/events.mux`);
  const t = setTimeout(() => reject(new Error("ws timeout (45s)")), 45000);
  ws.addEventListener("message", (e) => {
    frames.push(JSON.parse(e.data.toString()));
    const evs = frames.filter(f => f.method === "session/event");
    if (evs.some(f => f.payload.event.type === "turn/end")) { clearTimeout(t); ws.close(); resolve(); }
  });
  ws.addEventListener("error", () => reject(new Error("ws error")));
  ws.addEventListener("open", async () => {
    const pr = await rpc("session.prompt", { sessionId: sid, content: [{ type: "text", text: "用一句话自我介绍，然后返回 OK" }] });
    if (!pr.result.ok) { clearTimeout(t); reject(new Error("prompt rejected: " + JSON.stringify(pr.result.error))); }
  });
});
try { await wsDone; } catch (e) { console.log("FAIL ws flow:", e.message); failures++; }
const events = frames.filter(f => f.method === "session/event");
const seqs = events.map(f => f.payload.event.seq);
ok("ws seq continuous", seqs.join(",") === Array.from({ length: seqs.length }, (_, i) => i).join(","), seqs.join(","));
const assistant = events.filter(f => f.payload.event.type === "assistant/message");
const chunks = events.filter(f => f.payload.event.type === "assistant/chunk");
const lastAssistant = assistant[assistant.length - 1];
const text = lastAssistant?.payload.event.data.message.content
  .filter(b => b.type === "text").map(b => b.text).join("") || "";
ok("assistant/message present", !!lastAssistant);
ok("real model text reply (non-mock)", text.length > 10 && !text.includes("model generation failed"),
  JSON.stringify(text.slice(0, 80)));
ok("streaming chunks before message", chunks.length > 0, `${chunks.length} chunks`);
ok("history turn complete", events.some(f => f.payload.event.type === "user/message")
  && events.some(f => f.payload.event.type === "step/start")
  && events.some(f => f.payload.event.type === "turn/end"));

console.log(failures === 0 ? "\n=== M3 REAL PROVIDER VERIFY PASS ===" : `\n=== M3 REAL PROVIDER VERIFY FAIL (${failures}) ===`);
process.exit(failures === 0 ? 0 : 1);
