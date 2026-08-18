// 核心收尾验证：settingsNs 热替换（registerAdapter 写面）——provider 的 settings ns
// 动态 baseURL + credentials.set 动态 API key，写后下一请求即生效（无重启/无重新注册）。
// 前置：本地 mock SSE 端点 A/B（node 内置 http）+ keyless config.toml 装配。
// 用法：node scripts/hot-replace-verify.mjs
import { spawn } from "node:child_process";
import http from "node:http";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const PORT = 3082;
const API = `http://127.0.0.1:${PORT}/api`;
const work = mkdtempSync(join(tmpdir(), "hotreplace-"));
let failures = 0;
const ok = (name, cond, extra = "") => { console.log(cond ? "PASS" : "FAIL", name, extra); if (!cond) failures++; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function rpc(method, payload, rpcId = "hr") {
  const r = await fetch(`${API}/${method}`, { method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ type: "client-request", rpcId, method, payload }) });
  return r.json();
}

// ---- 本地 mock SSE 端点：记录收到的 authorization 头 ----
function mockEndpoint(port) {
  const seen = [];
  const server = http.createServer((req, res) => {
    seen.push({ url: req.url, authorization: req.headers.authorization || null });
    res.writeHead(200, { "content-type": "text/event-stream" });
    res.write('data: {"choices":[{"delta":{"role":"assistant","content":"hi"},"index":0}]}\n\n');
    res.write("data: [DONE]\n\n");
    res.end();
  });
  return new Promise((resolve) => {
    server.listen(port, "127.0.0.1", () => resolve({ server, seen }));
  });
}

async function historyTailError(sid) {
  const h = await rpc("session.history", { sessionId: sid });
  const events = h.result.value.events || [];
  // 只取最后一个 turn/end 的错误（history 追加式，旧回合错误不应误报）。
  let last = null;
  for (const ev of events) {
    const inner = ev.event || ev;
    if (inner.type === "turn/end") {
      const data = inner.data || {};
      last = data.reason && data.reason.error ? data.reason.error : null;
    }
  }
  return last;
}

async function main() {
  const A = await mockEndpoint(3190);
  const B = await mockEndpoint(3191);

  // keyless 装配：无 api_key → provider 仍装配，请求 MISSING_CREDENTIAL 直到 key 热补。
  const cfgPath = join(work, "config.toml");
  writeFileSync(cfgPath, `default_provider = "deepseek"\ndefault_model = "deepseek-chat"\n\n[[providers]]\nid = "deepseek"\nname = "DeepSeek"\nkind = "deepseek"\nbase_url = "http://127.0.0.1:3190/v1"\nmodels = ["deepseek-chat"]\n`);

  const db = join(work, "hot.db");
  const child = spawn("kernel/target/release/web-server.exe",
    ["--db", db, "--port", String(PORT), "--dist", "kernel/web-server/frontend", "--config", cfgPath],
    { stdio: ["ignore", "pipe", "pipe"] });
  try {
    const deadline = Date.now() + 20000;
    while (Date.now() < deadline) {
      if (child.exitCode !== null) throw new Error(`server exited ${child.exitCode}`);
      try {
        const r = await fetch(`${API}/host.describe`, { method: "POST", headers: { "content-type": "application/json" },
          body: JSON.stringify({ type: "client-request", rpcId: "probe", method: "host.describe", payload: {} }) });
        if (r.status === 200) break;
      } catch {}
      await sleep(300);
    }
  } catch (e) {
    console.log("FAIL startup:", e.message);
    process.exit(1);
  }

  const s = await rpc("session.create", {});
  const sid = s.result.value.sessionId;
  ok("session.create", s.result.ok, sid);

  // 1. keyless 首回合 → MISSING_CREDENTIAL（不装死、不静默）。
  const p1 = await rpc("session.prompt", { sessionId: sid, content: [{ type: "text", text: "hello" }] });
  ok("prompt accepted", p1.result.ok && p1.result.value.accepted === true);
  await sleep(1200);
  const e1 = await historyTailError(sid);
  ok("keyless prompt fails MISSING_CREDENTIAL", e1 && e1.code === "MISSING_CREDENTIAL", JSON.stringify(e1));
  ok("keyless never hit endpoint A", A.seen.length === 0, `A.seen=${A.seen.length}`);

  // 2. credentials.set DEEPSEEK_API_KEY → 下一请求即带新 key 打 A。
  const cs = await rpc("credentials.set", { ref: "DEEPSEEK_API_KEY", value: "hot-key-1" });
  ok("credentials.set", cs.result.ok);
  const p2 = await rpc("session.prompt", { sessionId: sid, content: [{ type: "text", text: "hi" }] });
  ok("prompt 2 accepted", p2.result.ok);
  await sleep(1200);
  ok("hot key hit endpoint A", A.seen.length === 1 && A.seen[0].authorization === "Bearer hot-key-1",
    JSON.stringify(A.seen));
  const e2 = await historyTailError(sid);
  ok("prompt 2 completed (no tail error)", e2 === null, JSON.stringify(e2));

  // 3. settings.update llm.deepseek baseURL → 下一请求改打 B。
  const su = await rpc("settings.update", { ns: "llm.deepseek", patch: { baseURL: "http://127.0.0.1:3191/v1" } });
  ok("settings.update llm.deepseek baseURL", su.result.ok && su.result.value.ns === "llm.deepseek");
  const p3 = await rpc("session.prompt", { sessionId: sid, content: [{ type: "text", text: "hi again" }] });
  ok("prompt 3 accepted", p3.result.ok);
  await sleep(1200);
  ok("prompt 3 hit endpoint B", B.seen.length === 1 && B.seen[0].authorization === "Bearer hot-key-1",
    JSON.stringify(B.seen));
  ok("prompt 3 did not hit A again", A.seen.length === 1);

  // 4. settings.describe 含 llm.deepseek ns。
  const sd = await rpc("settings.describe", {});
  const nsFound = (sd.result.value.namespaces || []).some((n) => n.ns === "llm.deepseek");
  ok("settings.describe lists llm.deepseek ns", nsFound, JSON.stringify((sd.result.value.namespaces || []).map((n) => n.ns)));

  // 5. 回归：未知 ns → settings-rejected；非法 credentials ref → bad-request。
  const sbad = await rpc("settings.update", { ns: "nope", patch: {} });
  ok("unknown ns settings-rejected", !sbad.result.ok && sbad.result.error.code === "settings-rejected",
    JSON.stringify(sbad.result.error));
  const cbad = await rpc("credentials.set", { ref: "BAD REF!", value: "k" });
  ok("invalid credential ref bad-request", !cbad.result.ok && cbad.result.error.code === "bad-request");

  console.log(failures === 0 ? "\nALL PASS" : `\n${failures} FAILURES`);
  child.kill();
  rmSync(work, { recursive: true, force: true });
  process.exit(failures === 0 ? 0 : 1);
}

main().catch((e) => { console.error(e); process.exit(1); });
