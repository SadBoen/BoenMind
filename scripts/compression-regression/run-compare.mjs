// 对比测试驱动：创建会话 → 按固定序列逐轮 POST /api/chat → 消费 SSE → 收集回复。
// 用法：node run-compare.mjs --group A|B|C [--base http://127.0.0.1:17322] [--rounds N] [--resume]
//       环境变量 OUT_DIR 覆盖输出目录（默认 out/）
//       --rounds N：只跑 ROUNDS 前 N 条（默认 30）；会话标题含实际轮数（避免与历史同名会话混淆）
// 输出：{OUT_DIR}/{group}.jsonl（每轮一行 JSON：round/user/reply/toolCalls/elapsedMs/ts）
// 会话 id 打印到 stdout，供日志关联。
//
// 组别约定（与 config.toml 配合）：
//   A = 装前基线：enabled_plugins 不含 ctx-compactor + [compaction] enabled = false
//   C = 只水线：  enabled_plugins 不含 ctx-compactor（compaction 默认开启）
//   B = 完整功能：enabled_plugins 含 ctx-compactor（compaction 默认开启）← 默认回归目标

import { writeFileSync, mkdirSync, readFileSync, existsSync } from "node:fs";
import { ROUNDS } from "./rounds.mjs";

const args = process.argv.slice(2);
const group = args.includes("--group") ? args[args.indexOf("--group") + 1] : null;
const base = args.includes("--base") ? args[args.indexOf("--base") + 1] : "http://127.0.0.1:17322";
const roundsArg = args.includes("--rounds") ? Number(args[args.indexOf("--rounds") + 1]) : 30;
const resume = args.includes("--resume");
const OUT_DIR = process.env.OUT_DIR ?? "out";
const ROUND_TIMEOUT_MS = 15 * 60 * 1000;

if (!group) {
  console.error("用法: node run-compare.mjs --group A|B|C [--base URL] [--rounds N] [--resume]");
  process.exit(1);
}
if (!Number.isInteger(roundsArg) || roundsArg < 1 || roundsArg > ROUNDS.length) {
  console.error(`--rounds 须为 1..${ROUNDS.length} 的整数，收到: ${roundsArg}`);
  process.exit(1);
}
// 取 ROUNDS 前 N 条；会话标题带实际轮数（如"对比-B-12轮"），与历史 30 轮会话区分
const ROUND_LIST = ROUNDS.slice(0, roundsArg);
const sessionTitle = `对比-${group}-${roundsArg}轮`;
mkdirSync(OUT_DIR, { recursive: true });
const outFile = `${OUT_DIR}/${group}.jsonl`;

const done = new Map(); // round -> record
if (resume && existsSync(outFile)) {
  for (const line of readFileSync(outFile, "utf8").trim().split("\n")) {
    if (!line) continue;
    const rec = JSON.parse(line);
    done.set(rec.round, rec);
  }
  console.log(`[resume] 已存在 ${done.size} 轮，从第 ${done.size + 1} 轮继续`);
}

// 创建会话（仅首次）
let sessionId = null;
if (done.size === 0) {
  const res = await fetch(`${base}/api/sessions`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ title: sessionTitle }),
  });
  if (!res.ok) throw new Error(`创建会话失败: ${res.status} ${await res.text()}`);
  sessionId = (await res.json()).id;
  console.log(`[session] ${sessionId} group=${group}`);
} else {
  // resume：从会话列表里找同名会话（取最新的）
  const res = await fetch(`${base}/api/sessions`);
  const sessions = await res.json();
  const mine = sessions.filter((s) => s.title === sessionTitle).sort((a, b) => b.created_at - a.created_at);
  if (mine.length === 0) throw new Error("resume 但找不到会话");
  sessionId = mine[0].id;
  console.log(`[resume session] ${sessionId}`);
}

// SSE 消费：POST /api/chat，逐行解析 data: {...}
async function chatOnce(round, message) {
  const started = Date.now();
  const res = await fetch(`${base}/api/chat`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ session_id: sessionId, message }),
  });
  if (!res.ok) throw new Error(`chat 失败: ${res.status} ${await res.text()}`);
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  let reply = "";
  let toolCalls = 0;
  let errorMsg = null;
  let finished = false;
  while (true) {
    const { done: eof, value } = await reader.read();
    if (eof) break;
    buf += decoder.decode(value, { stream: true });
    let idx;
    while ((idx = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, idx).trim();
      buf = buf.slice(idx + 1);
      if (!line.startsWith("data:")) continue;
      let ev;
      try {
        ev = JSON.parse(line.slice(5).trim());
      } catch {
        continue;
      }
      if (ev.type === "textDelta") reply += ev.delta ?? "";
      else if (ev.type === "toolCallStart") toolCalls++;
      else if (ev.type === "error") errorMsg = ev.message ?? "未知错误";
      else if (ev.type === "done") finished = true;
    }
    if (finished) break;
  }
  if (errorMsg) throw new Error(`SSE error: ${errorMsg}`);
  if (!finished) throw new Error(`未收到 done（可能超时/断连），round=${round}`);
  // 回复记录上限（§〇·五 21②）：超限工具结果若被模型回显进回复，
  // 截断留头尾再落 jsonl——记录只服务 analyze 轮次对齐与记忆检验，
  // 完整回复无价值，防 207MB 级回显撑爆 out 记录
  const REPLY_CAP = 200_000;
  if (reply.length > REPLY_CAP) {
    reply = `${reply.slice(0, REPLY_CAP)}\n[…回复过长已截断：原始 ${reply.length} 字符…]\n${reply.slice(-REPLY_CAP)}`;
  }
  return { reply, toolCalls, elapsedMs: Date.now() - started };
}

for (let round = 1; round <= ROUND_LIST.length; round++) {
  if (done.has(round)) continue;
  const message = ROUND_LIST[round - 1];
  let record = null;
  let attempts = 0;
  while (attempts < 3) {
    attempts++;
    try {
      const r = await Promise.race([
        chatOnce(round, message),
        new Promise((_, rej) => setTimeout(() => rej(new Error(`round ${round} 超时 ${ROUND_TIMEOUT_MS / 1000}s`)), ROUND_TIMEOUT_MS)),
      ]);
      record = { round, user: message, reply: r.reply, toolCalls: r.toolCalls, elapsedMs: r.elapsedMs, ts: Date.now() };
      break;
    } catch (err) {
      console.error(`[round ${round}] 尝试 ${attempts} 失败: ${err.message}`);
      if (attempts < 3) {
        const wait = 10_000 * attempts;
        console.error(`  等待 ${wait / 1000}s 后重试`);
        await new Promise((r) => setTimeout(r, wait));
      }
    }
  }
  if (!record) {
    console.error(`[round ${round}] 3 次尝试均失败，退出（可 --resume 续跑）`);
    process.exit(2);
  }
  done.set(round, record);
  writeFileSync(outFile, [...done.values()].sort((a, b) => a.round - b.round).map((r) => JSON.stringify(r)).join("\n") + "\n");
  console.log(`[round ${round}/${ROUND_LIST.length}] done. reply=${record.reply.length}ch toolCalls=${record.toolCalls} elapsed=${(record.elapsedMs / 1000).toFixed(0)}s`);
  await new Promise((r) => setTimeout(r, 2000)); // 轮间 2s 缓冲
}

console.log(`\n[all done] session=${sessionId} group=${group} 输出: ${outFile}`);
