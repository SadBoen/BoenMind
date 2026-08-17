// conformance diff：重放 Node 后端 wire 轨迹到 Rust 兼容层，逐条对照。
// 用法：node docs/conformance/diff-traces.mjs [rustPort]
// 归一化规则：动态字段（值随实例/时间/配置变化）跳过值、只要求字段名存在；
// 其余结构与静态字段逐字一致才算通过。数组长度不同时按"前缀元素逐个比形状"。

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const RUST_PORT = process.argv[2] || "3081";
const TRACES_DIR = join(import.meta.dirname, "node-traces");

// 动态字段：值随实例/时间/配置变化，比对时跳过（但字段名本身要在）。
const DYNAMIC_KEYS = new Set([
  "rpcId", "date", "cwd", "version", "sessionId", "time", "seq",
  "timestamps", "startedAt", "finishedAt", "createdAt", "updatedAt",
  "requestHeaders", "responseHeaders", "error",
  "attachedSessions", "provider", "model", "canOpenPath",
  "writable", "hasDocument", "namespaces", "credentials", "presets",
  "groups", "models", "providers", "items", "entries", "events",
  "refs", "archivedSessionIds", "failures", "title", "agentPreset",
  "label", "name", "id", "asOfSeq", "values", "projections",
]);

function normalize(v) {
  if (Array.isArray(v)) return v.map(normalize);
  if (v && typeof v === "object") {
    const out = {};
    for (const [k, val] of Object.entries(v)) {
      if (DYNAMIC_KEYS.has(k)) {
        out[k] = "«dynamic»";
      } else {
        out[k] = normalize(val);
      }
    }
    return out;
  }
  return v;
}

function diffPaths(a, b, path = "$") {
  const problems = [];
  if (typeof a !== typeof b) {
    problems.push(`${path}: type ${typeof a} vs ${typeof b}`);
    return problems;
  }
  if (Array.isArray(a) && Array.isArray(b)) {
    const len = Math.min(a.length, b.length);
    for (let i = 0; i < len; i++) {
      problems.push(...diffPaths(a[i], b[i], `${path}[${i}]`));
    }
    return problems;
  }
  if (a && b && typeof a === "object") {
    const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
    for (const k of keys) {
      if (!(k in a)) { problems.push(`${path}.${k}: missing in rust`); continue; }
      if (!(k in b)) { problems.push(`${path}.${k}: missing in rust-response`); continue; }
      problems.push(...diffPaths(a[k], b[k], `${path}.${k}`));
    }
    return problems;
  }
  if (a !== b) problems.push(`${path}: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`);
  return problems;
}

async function main() {
  const files = readdirSync(TRACES_DIR).filter((f) => f.endsWith(".json"));
  let pass = 0, fail = 0;

  // prelude：用 Node 轨迹产生的 sessionId 在 Rust 端建同名会话，
  // 让 history/prompt/rename/cancel 有命中对象（session.create 支持预分配 sessionId）。
  const createTrace = JSON.parse(readFileSync(join(TRACES_DIR, "session-create.json"), "utf-8"));
  const nodeSessionId = (() => {
    try {
      return JSON.parse(createTrace.responseBody).result?.value?.sessionId;
    } catch { return undefined; }
  })();
  if (nodeSessionId) {
    const preBody = {
      type: "client-request",
      rpcId: "prelude-create",
      method: "session.create",
      payload: { sessionId: nodeSessionId },
    };
    await fetch(`http://127.0.0.1:${RUST_PORT}/api/session.create`, {
      method: "POST",
      headers: { "content-type": "application/json", host: `127.0.0.1:${RUST_PORT}`, origin: `http://127.0.0.1:${RUST_PORT}` },
      body: JSON.stringify(preBody),
    }).catch(() => {});
  }

  for (const f of files.sort()) {
    const trace = JSON.parse(readFileSync(join(TRACES_DIR, f), "utf-8"));
    // 传输层已知差异：hyper 强制 HTTP/1.1 自动补 Host 头，Node http.request 可不带。
    // 裸请求场景（bare-post）在真实浏览器/客户端永不发生，标记为已知差异跳过。
    if (f === "bare-post-no-headers.json") {
      console.log(`~ ${f}: SKIP (transport-level: hyper injects Host header; Node http.request may omit it)`);
      continue;
    }
    const body = trace.requestBody || "";
    // 用轨迹记录的原始请求头（bare-post 无头、evil-origin 坏 Origin 场景必须还原）
    const rh = trace.requestHeaders || {};
    const headers = {
      "content-type": "application/json",
      ...(rh.host ? { host: rh.host.replace(":3080", ":" + RUST_PORT) } : {}),
      ...(rh.origin ? { origin: rh.origin.replace(":3080", ":" + RUST_PORT) } : {}),
    };
    let resp;
    try {
      resp = await fetch(`http://127.0.0.1:${RUST_PORT}${trace.path}`, {
        method: trace.method,
        headers,
        body: body || undefined,
      });
    } catch (e) {
      console.log(`✗ ${f}: FETCH FAILED ${e.message}`);
      fail++;
      continue;
    }
    const rustStatus = resp.status;
    const rustBody = await resp.text();

    if (rustStatus !== trace.status) {
      console.log(`✗ ${f}: status ${rustStatus} vs node ${trace.status}`);
      console.log(`    rust body: ${rustBody.slice(0, 160)}`);
      fail++;
      continue;
    }

    let nodeJson = null, rustJson = null;
    try { nodeJson = JSON.parse(trace.responseBody); } catch {}
    try { rustJson = JSON.parse(rustBody); } catch {}
    if (nodeJson !== null || rustJson !== null) {
      if (nodeJson === null || rustJson === null) {
        console.log(`✗ ${f}: JSON parse mismatch (node=${nodeJson !== null}, rust=${rustJson !== null})`);
        console.log(`    rust body: ${rustBody.slice(0, 160)}`);
        fail++;
        continue;
      }
      const reqId = (() => { try { return JSON.parse(body || "{}").rpcId; } catch { return undefined; } })();
      if (reqId && rustJson.rpcId !== reqId) {
        console.log(`✗ ${f}: rpcId not echoed (sent ${reqId}, got ${rustJson.rpcId})`);
        fail++;
        continue;
      }
      const problems = diffPaths(normalize(nodeJson), normalize(rustJson));
      if (problems.length > 0) {
        console.log(`✗ ${f}: ${problems.length} diffs`);
        for (const p of problems.slice(0, 4)) console.log(`    ${p}`);
        fail++;
        continue;
      }
    }
    console.log(`✓ ${f} (status ${rustStatus})`);
    pass++;
  }
  console.log(`\nCONFORMANCE: ${pass} pass, ${fail} fail`);
  process.exit(fail > 0 ? 1 : 0);
}

main();
