// 分析：从 server-{group}.log 提取 bm.prompt_usage，按 {OUT_DIR}/{group}.jsonl 的轮次时间窗对齐，
// 输出每轮的上下文规模（input+cache_read=发送量）、计费输入、输出、累计。
// 用法：node analyze.mjs --group A|B|C [--log server-A.log] [--out out/A.jsonl]
//       环境变量 OUT_DIR 覆盖输出目录（默认 out/）

import { readFileSync, existsSync } from "node:fs";

const args = process.argv.slice(2);
const group = args.includes("--group") ? args[args.indexOf("--group") + 1] : null;
const OUT_DIR = process.env.OUT_DIR ?? "out";
const logFile = args.includes("--log") ? args[args.indexOf("--log") + 1] : `server-${group}.log`;
const outFile = args.includes("--out") ? args[args.indexOf("--out") + 1] : `${OUT_DIR}/${group}.jsonl`;
if (!group) {
  console.error("用法: node analyze.mjs --group A|B|C");
  process.exit(1);
}

// 1. 解析日志 usage 行
const usageLines = [];
for (const rawLine of readFileSync(logFile, "utf8").split("\n")) {
  if (!rawLine.includes("bm.prompt_usage")) continue;
  const line = rawLine.replace(/\x1b\[[0-9;]*m/g, ""); // 剥离 ANSI 颜色码
  // 时间戳格式: 2026-08-12T01:11:46.926454Z
  const tsMatch = line.match(/(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z)/);
  const ts = tsMatch ? new Date(tsMatch[1]).getTime() : null;
  const m = line.match(/input=(\d+) output=(\d+) cache_read=(\d+) cache_write=(\d+) total=(\d+) session_total=(\d+)/);
  if (!m || !ts) continue;
  usageLines.push({
    ts,
    input: +m[1], output: +m[2], cache_read: +m[3], cache_write: +m[4],
    total: +m[5], session_total: +m[6],
    sent: +m[1] + +m[3], // 发送到模型的上下文量（新输入+缓存命中）
  });
}

// 2. 读取轮次记录
const rounds = [];
if (existsSync(outFile)) {
  for (const line of readFileSync(outFile, "utf8").trim().split("\n")) {
    if (line) rounds.push(JSON.parse(line));
  }
}
if (rounds.length === 0) {
  console.error(`out 文件为空: ${outFile}`);
  process.exit(1);
}

// 3. 按轮次执行窗口对齐 usage（ts 为完成时间，反推开始 = ts - elapsedMs）
console.log(`=== 组 ${group}：${rounds.length} 轮，usage 行 ${usageLines.length} ===`);
console.log("轮次 | 发送量(峰值) | 输入 | 输出 | 缓存读 | 每轮耗时 | 上下文规模趋势");
const perRound = [];
for (let i = 0; i < rounds.length; i++) {
  const r = rounds[i];
  const start = r.ts - (r.elapsedMs ?? 0) - 2000; // 轮次开始（含 2s 缓冲）
  const end = r.ts + 2000; // 轮次完成
  const lines = usageLines.filter((u) => u.ts >= start && u.ts < end);
  if (lines.length === 0) {
    perRound.push({ round: r.round, sent: null });
    console.log(`${r.round.toString().padStart(2)} | (无 usage 日志)`);
    continue;
  }
  const last = lines[lines.length - 1]; // 该轮最后一条 assistant 消息 = 上下文最终规模
  const peak = Math.max(...lines.map((u) => u.sent));
  perRound.push({
    round: r.round,
    sent: last.sent,
    peak,
    input: last.input,
    output: last.output,
    cache_read: last.cache_read,
    elapsed: r.elapsedMs,
  });
  console.log(
    `${r.round.toString().padStart(2)} | 峰值 ${String(peak).padStart(7)} | 终值 ${String(last.sent).padStart(7)} | 输入 ${String(last.input).padStart(6)} | 输出 ${String(last.output).padStart(5)} | 缓存 ${String(last.cache_read).padStart(7)} | ${(r.elapsedMs / 1000).toFixed(0)}s`,
  );
}

// 4. 汇总
const valid = perRound.filter((p) => p.sent != null);
const sumSent = valid.reduce((a, b) => a + b.sent, 0);
const sumPeak = valid.reduce((a, b) => a + b.peak, 0);
const sumOutput = valid.reduce((a, b) => a + (b.output ?? 0), 0);
const sumInput = valid.reduce((a, b) => a + (b.input ?? 0), 0);
const sumCache = valid.reduce((a, b) => a + (b.cache_read ?? 0), 0);
const totalTime = rounds.reduce((a, b) => a + (b.elapsedMs ?? 0), 0);
const maxCtx = Math.max(...valid.map((p) => p.sent));
console.log("\n=== 汇总 ===");
console.log(`累计发送量 (∑ input+cache_read): ${(sumSent / 1000).toFixed(1)}K tokens`);
console.log(`累计峰值发送量: ${(sumPeak / 1000).toFixed(1)}K`);
console.log(`累计计费输入 (∑ input): ${(sumInput / 1000).toFixed(1)}K`);
console.log(`累计输出: ${(sumOutput / 1000).toFixed(1)}K`);
console.log(`累计缓存命中: ${(sumCache / 1000).toFixed(1)}K`);
console.log(`末轮上下文: ${(valid[valid.length - 1].sent / 1000).toFixed(1)}K，全程峰值: ${(maxCtx / 1000).toFixed(1)}K`);
console.log(`总耗时: ${(totalTime / 1000 / 60).toFixed(1)} 分钟`);

// 5. 压缩触发检测：发送量相对上一轮回落 > 40%（且上一轮 > 20K）视为发生压缩
console.log("\n=== 压缩触发检测（发送量较上轮回落 >40%）===");
for (let i = 1; i < valid.length; i++) {
  const prev = valid[i - 1].sent;
  const cur = valid[i].sent;
  if (prev > 20_000 && cur < prev * 0.6) {
    console.log(`第 ${valid[i].round} 轮: ${(prev / 1000).toFixed(1)}K → ${(cur / 1000).toFixed(1)}K（回落 ${(100 - (cur / prev) * 100).toFixed(0)}%）`);
  }
}
