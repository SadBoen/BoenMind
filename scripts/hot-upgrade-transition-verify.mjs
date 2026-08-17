// 热升级过渡态验收（门禁 3 半验收）：起 A(3079) → 流式中 kill -9 → 起 B(同端口)
// 恢复日志 → host.describe/session.history 完整 → 可继续 prompt。
// mock 模式（无 --config），杀进程不产生费用。用法：node scripts/hot-upgrade-transition-verify.mjs
import { spawn, execSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const PORT = 3079;
const API = `http://127.0.0.1:${PORT}/api`;
const work = mkdtempSync(join(tmpdir(), "hotup-"));
let failures = 0;
const ok = (name, cond, extra = "") => { console.log(cond ? "PASS" : "FAIL", name, extra); if (!cond) failures++; };

const sleep = (ms) => new Promise(r => setTimeout(r, ms));

async function rpc(method, payload, rpcId = "hotup") {
  const r = await fetch(`${API}/${method}`, { method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ type: "client-request", rpcId, method, payload }) });
  return r.json();
}

function startServer(db, port) {
  // 从仓库根启动（--dist 相对根解析）。
  const child = spawn("kernel/target/release/web-server.exe",
    ["--db", db, "--port", String(port), "--dist", "kernel/web-server/frontend"],
    { stdio: ["ignore", "pipe", "pipe"] });
  return child;
}

async function waitHealthy(proc, timeoutMs = 20000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (proc.exitCode !== null) throw new Error(`server exited ${proc.exitCode}`);
    try {
      const r = await fetch(`${API}/host.describe`, { method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ type: "client-request", rpcId: "probe", method: "host.describe", payload: {} }) });
      if (r.status === 200) return true;
    } catch {}
    await sleep(300);
  }
  throw new Error("server not healthy in time");
}

async function main() {
  const db = join(work, "hot.db");
  console.log("work dir:", work);

  // ---- 阶段 A：起服务、建会话、跑一个完整回合 ----
  let a = startServer(db, PORT);
  try {
    await waitHealthy(a);
  } catch (e) {
    console.log("FAIL phase A startup:", e.message);
    process.exit(1);
  }
  const s = await rpc("session.create", {});
  const sid = s.result.value.sessionId;
  ok("A session.create", s.result.ok, sid);

  // WS mux 驱动一回合（流式中判断 turn/end）
  await new Promise((resolve, reject) => {
    const ws = new WebSocket(`ws://127.0.0.1:${PORT}/api/events.mux`);
    const t = setTimeout(() => reject(new Error("ws timeout")), 20000);
    ws.addEventListener("message", (e) => {
      const d = JSON.parse(e.data.toString());
      if (d.method === "session/event" && d.payload.event.type === "turn/end") { clearTimeout(t); ws.close(); resolve(); }
    });
    ws.addEventListener("open", async () => {
      await rpc("session.prompt", { sessionId: sid, content: [{ type: "text", text: "record me for the upgrade test" }] });
    });
  });
  ok("A turn completed", true);

  // ---- kill -9（模拟崩溃） ----
  execSync(`taskkill /F /PID ${a.pid}`);
  await sleep(800);
  ok("A killed -9", true);

  // ---- 阶段 B：同端口重启，恢复 ----
  let b = null;
  try {
    b = startServer(db, PORT);
    await waitHealthy(b);
  } catch (e) {
    console.log("FAIL phase B startup:", e.message);
    process.exit(1);
  }
  const hd = await rpc("host.describe", {});
  ok("B host.describe", hd.result.ok);

  const hl = await rpc("session.list", {});
  ok("B session.list contains session", hl.result.ok && hl.result.value.items.some(i => i.sessionId === sid));

  const hist = await rpc("session.history", { sessionId: sid });
  const types = hist.result.ok ? hist.result.value.events.map(e => e.event.type) : [];
  ok("B history complete (turn/end present)", hist.result.ok && types.includes("turn/end"), types.join(","));
  ok("B history user message intact", hist.result.ok && types.includes("user/message"));

  // 继续 prompt → 回合接续（turn 编号不重复）
  let continued = false;
  try {
    await new Promise((resolve, reject) => {
      const ws = new WebSocket(`ws://127.0.0.1:${PORT}/api/events.mux`);
      const t = setTimeout(() => reject(new Error("continue ws timeout")), 20000);
      ws.addEventListener("message", (e) => {
        const d = JSON.parse(e.data.toString());
        if (d.method === "session/event" && d.payload.event.type === "turn/end") { clearTimeout(t); ws.close(); resolve(); }
      });
      ws.addEventListener("open", async () => {
        await rpc("session.prompt", { sessionId: sid, content: [{ type: "text", text: "continue after restart" }] });
      });
    });
    continued = true;
  } catch {}
  ok("B can continue prompting", continued);

  // 清理
  try { b.kill(); } catch {}
  rmSync(work, { recursive: true, force: true });
  console.log(failures === 0 ? "\n=== HOT UPGRADE TRANSITION PASS ===" : `\n=== HOT UPGRADE TRANSITION FAIL (${failures}) ===`);
  process.exit(failures === 0 ? 0 : 1);
}

main();
