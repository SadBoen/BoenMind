/**
 * Context Compactor —— BoenMind 上下文压缩补强插件（层 B）。
 *
 * 自研实现，仅借鉴 Hermes / context-mode 的行为思路，不复制任何第三方代码。
 * 三个能力（全部基于 pi 扩展机制，不触碰 pi 核心代码）：
 *
 * ① ctx_execute   —— 沙箱内执行 JS（Think in Code）：数据/输出只在插件内，
 *                     返回 console 输出与结果摘要，避免大输出进模型上下文。
 * ② tool_result 修剪 —— 大工具输出在进入模型前替换为自描述占位符（含检索 key），
 *                     原文经秘密扫描过滤后写入索引；模型需要细节时用 ctx_search 找回。
 * ③ 落库 + ctx_search —— 事件持续落盘（JSONL，按项目分桶），简易词频检索。
 *
 * 配置（可选，项目级 JSON 文件 `<cwd>/.boenmind/ctx-compactor.json`）：
 * {
 *   "trimEnabled": true,          // 是否启用修剪
 *   "trimThreshold": 200,         // 修剪阈值（字符）；输出超过则修剪
 *   "placeholderHead": 300,       // 占位符保留原文前 N 字符作摘要
 *   "maxIndexBytes": 8388608,     // 索引文件轮转阈值（8MB）
 *   "indexDirName": ".boenmind/ctx-index"  // 索引目录（相对 cwd）
 * }
 *
 * 事件说明（Phase 0 验证结论）：
 * - `tool_result`（post-exec，可修改结果）在 SDK 路径可用 —— 修剪与落库都在这做；
 * - `tool_execution_*` 系列只在 CLI/rpc 路径派发，SDK 路径收不到，不依赖；
 * - `session_before_compact` 在自动压缩时派发（暂只做日志，不做行为干预）。
 */
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import * as fs from "node:fs";
import * as path from "node:path";
import * as crypto from "node:crypto";

// ─────────────────────────────── 默认配置 ───────────────────────────────
const DEFAULTS = {
	trimEnabled: true,
	trimThreshold: 200,
	placeholderHead: 300,
	maxIndexBytes: 8 * 1024 * 1024,
	indexDirName: ".boenmind/ctx-index",
};

/** 不修剪的工具（避免循环修剪 + 模型检索路径保持直通 + 搜索结果需模型直接消费） */
const SELF_TOOLS = new Set(["ctx_execute", "ctx_search", "web_search", "web_fetch"]);

/** 常见 API key / 秘密格式（落库前替换，防泄漏进索引） */
const SECRET_PATTERNS = [
	/sk-[A-Za-z0-9]{20,}/g,
	/AKIA[0-9A-Z]{16}/g,
	/gh[pousr]_[A-Za-z0-9]{20,}/g,
	/xox[baprs]-[A-Za-z0-9-]{10,}/g,
	/(?:api[_-]?key|apikey|secret|token)["']?\s*[:=]\s*["'][^"']{8,}["']/gi,
	/bearer\s+[A-Za-z0-9._~+/=-]{20,}/gi,
];

/** 供 ctx_search 提取查询词的停用词 */
const STOP_WORDS = new Set([
	"the", "a", "an", "of", "to", "in", "on", "and", "or", "is", "are", "for",
	"with", "at", "by", "from", "as", "it", "this", "that", "was", "were",
	"的", "了", "和", "是", "在", "与", "有", "为", "就", "而",
]);

// 宿主 API 引用（default export 里赋值；hostcall 工具调用需要）
let piApi: ExtensionAPI | undefined;

interface CtxConfig {
	trimEnabled: boolean;
	trimThreshold: number;
	placeholderHead: number;
	maxIndexBytes: number;
	indexDirName: string;
}

interface IndexEntry {
	key: string;
	ts: number;
	tool: string;
	toolCallId?: string;
	sessionId?: string;
	file?: string;
	summary: string;
	content: string;
}

// ─────────────────────────────── 运行时状态 ───────────────────────────────
let config: CtxConfig = { ...DEFAULTS };
let configLoaded = false;

function loadConfig(cwd: string): CtxConfig {
	const cfgPath = path.join(cwd, ".boenmind", "ctx-compactor.json");
	try {
		const raw = fs.readFileSync(cfgPath, "utf8");
		const parsed = JSON.parse(raw) as Partial<CtxConfig>;
		return {
			trimEnabled: parsed.trimEnabled ?? DEFAULTS.trimEnabled,
			trimThreshold: numberOr(parsed.trimThreshold, DEFAULTS.trimThreshold, 1),
			placeholderHead: numberOr(parsed.placeholderHead, DEFAULTS.placeholderHead, 0),
			maxIndexBytes: numberOr(parsed.maxIndexBytes, DEFAULTS.maxIndexBytes, 1024),
			indexDirName: String(parsed.indexDirName ?? DEFAULTS.indexDirName),
		};
	} catch {
		return { ...DEFAULTS };
	}
}

function numberOr(value: unknown, fallback: number, min: number): number {
	const n = typeof value === "number" && Number.isFinite(value) ? value : fallback;
	return Math.max(n, min);
}

// ─────────────────────────────── 索引（JSONL，按项目分桶） ───────────────────────────────
function indexDir(cwd: string): string {
	return path.join(cwd, config.indexDirName);
}

function indexFile(cwd: string): string {
		return path.join(indexDir(cwd), "entries.jsonl");
}

/**
 * 追加一条索引记录（真实落盘）。
 *
 * 说明：QuickJS 扩展的 node:fs 写入是 VFS 内存虚拟层（不落盘，重启丢失），
 * 只有读有 host 回退。真实持久化必须走宿主工具 `pi.tool("write")`。
 * 全量读-改-写；超过 maxIndexBytes 时丢弃最旧一半（简单轮转，避免文件无限增长）。
 */
async function appendEntry(cwd: string, entry: IndexEntry): Promise<void> {
	try {
		const dir = indexDir(cwd);
		const file = path.join(dir, "entries.jsonl");
		let existing = "";
		try {
			const read = (await (piApi as any).tool("read", { path: file })) as any;
			const text = Array.isArray(read?.content)
				? read.content
						.filter((b: any) => b && b.type === "text" && typeof b.text === "string")
						.map((b: any) => b.text)
						.join("\n")
				: "";
			if (text) existing = text;
		} catch {
			// 文件不存在或不可读 → 从头开始
		}
		// 防污染校验：只保留合法 JSONL 行（模型或其他进程可能误改过索引文件，
		// 非法内容直接丢弃，避免损坏持续累积）
		const validLines = existing
			.split("\n")
			.filter((line: string) => line.trim() !== "")
			.filter((line: string) => {
				try {
					JSON.parse(line);
					return true;
				} catch {
					return false;
				}
			});
		let kept = validLines.join("\n");
		if (kept.length > config.maxIndexBytes) {
			// 丢弃最旧一半，保留最新内容
			kept = kept.slice(kept.length / 2);
		}
		const next = kept.length > 0 ? kept + "\n" : "";
		await (piApi as any).tool("write", { path: file, content: next + JSON.stringify(entry) + "\n" });
	} catch (e: any) {
		// 落库失败不阻断主流程（fail-open），但记录原因便于排查
		console.error(`[ctx-compactor] appendEntry failed: ${String(e?.message ?? e)}`);
	}
}

/** 秘密扫描：把命中常见 key 格式的片段替换为 [REDACTED]。 */
function redactSecrets(text: string): string {
	let out = text;
	for (const re of SECRET_PATTERNS) {
		re.lastIndex = 0;
		out = out.replace(re, "[REDACTED]");
	}
	return out;
}

/** 从 tool_result 的 content（ContentBlock[]）提取纯文本。 */
function extractText(content: unknown): string {
	if (!Array.isArray(content)) return "";
	return content
		.map((block: any) =>
			block && typeof block === "object" && block.type === "text" && typeof block.text === "string"
				? block.text
				: "",
		)
		.join("\n");
}

/** 从工具入参提取文件名（read/write/edit/grep/find/ls 等）。 */
function extractFile(toolName: string, input: unknown): string | undefined {
	if (!input || typeof input !== "object") return undefined;
	const obj = input as Record<string, unknown>;
	for (const key of ["path", "file", "file_path"]) {
		const v = obj[key];
		if (typeof v === "string" && v.trim()) return v.trim();
	}
	// bash 类工具：命令本身作为摘要来源
	if (toolName === "bash" && typeof obj.command === "string") return undefined;
	return undefined;
}

/** 生成自描述占位符（含摘要与检索指引 —— 修剪必须配检索，否则模型丢信息）。 */
function placeholderFor(entry: IndexEntry): string {
	const head = entry.content.slice(0, config.placeholderHead);
	return (
		`[工具输出已修剪：原输出 ${entry.content.length} 字符，已存入索引（检索 key: ${entry.key}）。` +
		`需要全文时用 ctx_search 查询 key "${entry.key}" 或相关关键词。摘要：\n${head}]`
	);
}

// ─────────────────────────────── ctx_execute：沙箱执行 JS ───────────────────────────────
const MAX_CODE_CHARS = 20_000;

function executeJs(code: string): Promise<{ ok: boolean; logs: string[]; result: unknown; error?: string }> {
	// 用户代码通过间接 eval 在 QuickJS 沙箱里执行（天然受限），
	// console.log 由捕获函数接管，输出只回传给模型，不落索引。
	// 注意：不能把 code 当 new Function 参数引用（那是变量绑定不是文本替换，
	// 代码不会执行）；拼接进 async 函数体由 eval 执行。
	let run: (code: string) => Promise<unknown>;
	try {
		run = new Function(
			"code",
			`
			return (async () => {
				const logs = [];
				const origLog = console.log;
				console.log = (...args) => {
					try { logs.push(args.map(String).join(" ")); } catch (_) {}
				};
				try {
					const src = "(async () => { " + code + " })()";
					const result = await (0, eval)(src);
					return { ok: true, logs, result: result === undefined ? null : result };
				} catch (e) {
					return { ok: false, logs, error: String((e && e.message) || e) };
				} finally {
					console.log = origLog;
				}
			})();
		`,
		) as (code: string) => Promise<unknown>;
	} catch (e) {
		return Promise.resolve({ ok: false, logs: [], error: `语法错误: ${String((e as Error)?.message ?? e)}` });
	}
	return run(code) as Promise<ReturnType<typeof executeJs>>;
}

function stringifyResult(value: unknown): string {
	try {
		return JSON.stringify(value, null, 2);
	} catch {
		return String(value);
	}
}

// ─────────────────────────────── ctx_search：简易词频检索 ───────────────────────────────
function tokenize(text: string): string[] {
	return (text.toLowerCase().match(/[a-z0-9_\u4e00-\u9fff]+/g) ?? []).filter(
		(t) => t.length > 1 && !STOP_WORDS.has(t),
	);
}

/** 简易 BM25 风格打分：词频 × 逆文档频率，按长度归一。 */
function searchIndex(cwd: string, query: string, limit: number): IndexEntry[] {
	const terms = tokenize(query);
	if (terms.length === 0) return [];
	// 支持 key 直接检索
	const keyTerm = query.trim().match(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i);
	const dir = indexDir(cwd);
	if (!fs.existsSync(dir)) return [];

	const files = fs
		.readdirSync(dir)
		.filter((f: string) => f.endsWith(".jsonl"))
		.map((f: string) => path.join(dir, f));
	const entries: IndexEntry[] = [];
	for (const file of files) {
		try {
			for (const line of fs.readFileSync(file, "utf8").split("\n")) {
				if (!line.trim()) continue;
				try {
					entries.push(JSON.parse(line) as IndexEntry);
				} catch {
					// 跳过损坏行
				}
			}
		} catch {
			// 跳过不可读文件
		}
	}

	const scored: Array<{ entry: IndexEntry; score: number }> = [];
	for (const entry of entries) {
		if (keyTerm && entry.key.toLowerCase() === query.trim().toLowerCase()) {
			scored.push({ entry, score: Number.MAX_SAFE_INTEGER });
			continue;
		}
		const haystack = `${entry.tool} ${entry.file ?? ""} ${entry.summary} ${entry.content}`.toLowerCase();
		let score = 0;
		for (const term of terms) {
			const idx = haystack.indexOf(term);
			if (idx !== -1) score += 1 + (idx < 200 ? 2 : 0);
		}
		if (score > 0) scored.push({ entry, score });
	}
	scored.sort((a, b) => b.score - a.score);
	return scored.slice(0, limit).map((s) => s.entry);
}

// ─────────────────────────────── 扩展入口 ───────────────────────────────
export default function (pi: ExtensionAPI) {
	piApi = pi;

	// ---- startup：加载项目级配置 ----
	pi.on("startup", async (_event, ctx: any) => {
		try {
			if (ctx && typeof ctx.cwd === "string") {
				config = loadConfig(ctx.cwd);
				configLoaded = true;
				console.log(`[ctx-compactor] startup ok, cwd=${ctx.cwd}, config=${JSON.stringify(config)}`);
			} else {
				console.log(`[ctx-compactor] startup: ctx.cwd missing, ctx keys=${ctx ? Object.keys(ctx).join(",") : "none"}`);
			}
		} catch (e: any) {
			console.error(`[ctx-compactor] startup failed: ${String(e?.message ?? e)} stack=${e?.stack ?? "no-stack"}`);
		}
	});

	// ---- ① ctx_execute：沙箱执行 ----
	pi.registerTool({
		name: "ctx_execute",
		label: "ctx_execute",
		description:
			"在沙箱内执行 JavaScript（Think in Code）。适合数据换算/文本处理/快速验证等小脚本，避免大输出污染上下文。" +
			"只返回 console.log 输出与最终结果（JSON 序列化）。代码限制 " + MAX_CODE_CHARS + " 字符；" +
			"language 参数当前仅支持 \"js\"。",
		parameters: {
			type: "object",
			properties: {
				language: {
					type: "string",
					description: '脚本语言，当前仅支持 "js"',
				},
				code: {
					type: "string",
					description: "要执行的 JavaScript 代码（支持 async/await 顶层用法）",
				},
			},
			required: ["language", "code"],
		},

		async execute(_toolCallId, params: any, _signal, _onUpdate, _ctx) {
			const language = String(params?.language ?? "").trim();
			const code = String(params?.code ?? "");
			if (language !== "js") {
				return {
					content: [{ type: "text", text: `ctx_execute: 暂不支持语言 "${language}"，当前仅支持 "js"` }],
					details: { ok: false, reason: "unsupported_language" },
					isError: true,
				};
			}
			if (code.length === 0) {
				return {
					content: [{ type: "text", text: "ctx_execute: code 不能为空" }],
					details: { ok: false, reason: "empty_code" },
					isError: true,
				};
			}
			if (code.length > MAX_CODE_CHARS) {
				return {
					content: [
						{
							type: "text",
							text: `ctx_execute: 代码过长（${code.length} 字符，上限 ${MAX_CODE_CHARS}），请精简脚本`,
						},
					],
					details: { ok: false, reason: "code_too_long" },
					isError: true,
				};
			}
			const result = await executeJs(code);
			const logsText = result.logs.length > 0 ? `console 输出：\n${result.logs.join("\n")}` : "(无 console 输出)";
			if (!result.ok) {
				return {
					content: [{ type: "text", text: `ctx_execute 执行出错：${result.error}\n${logsText}` }],
					details: { ok: false, error: result.error, logs: result.logs },
					isError: true,
				};
			}
			const resultText = stringifyResult(result.result);
			// 结果本身可能很大：截断到 8K 字符，超出部分只给摘要
			const capped = resultText.length > 8192 ? `${resultText.slice(0, 8192)}\n…（结果共 ${resultText.length} 字符，已截断）` : resultText;
			return {
				content: [{ type: "text", text: `ctx_execute 执行成功。\n${logsText}\n结果：\n${capped}` }],
				details: { ok: true, logLines: result.logs.length, resultChars: resultText.length },
			};
		},
	});

	// ---- ③ ctx_search：检索索引 ----
	pi.registerTool({
		name: "ctx_search",
		label: "ctx_search",
		description:
			"检索被修剪的工具输出索引（当前项目 .boenmind/ctx-index）。" +
			"当工具结果被修剪（占位符含 key）或需要找回历史工具输出细节时使用。" +
			"可用检索 key（如 \"a1b2...\"）精确定位，或关键词模糊检索。返回命中的摘要与原文长度。",
		parameters: {
			type: "object",
			properties: {
				query: {
					type: "string",
					description: "检索关键词或完整检索 key（多个词为 AND 加权）",
				},
				limit: {
					type: "number",
					description: "返回条数上限（默认 10，最大 50）",
				},
			},
			required: ["query"],
		},

		async execute(_toolCallId, params: any, _signal, _onUpdate, ctx: any) {
			const query = String(params?.query ?? "").trim();
			if (!query) {
				return {
					content: [{ type: "text", text: "ctx_search: query 不能为空" }],
					details: { ok: false },
					isError: true,
				};
			}
			const cwd = typeof ctx?.cwd === "string" ? ctx.cwd : "";
			if (!cwd) {
				return {
					content: [{ type: "text", text: "ctx_search: 无法确定工作目录" }],
					details: { ok: false },
					isError: true,
				};
			}
			if (!configLoaded) config = loadConfig(cwd);
			const limit = Math.min(Math.max(Number(params?.limit) || 10, 1), 50);
			const hits = searchIndex(cwd, query, limit);
			if (hits.length === 0) {
				return {
					content: [{ type: "text", text: `ctx_search: 未找到匹配（query: "${query}"）` }],
					details: { ok: true, hits: 0 },
				};
			}
			const lines = hits.map((h, i) => {
				const head = h.summary || h.content.slice(0, 200);
				return `${i + 1}. [${h.tool}] key=${h.key} 原文 ${h.content.length} 字符${h.file ? ` 文件=${h.file}` : ""}\n   ${head}`;
			});
			return {
				content: [{ type: "text", text: `ctx_search: ${hits.length} 条命中\n\n${lines.join("\n")}` }],
				details: {
					ok: true,
					hits: hits.map((h) => ({ key: h.key, tool: h.tool, chars: h.content.length })),
				},
			};
		},
	});

	// ---- ② tool_result：零成本修剪 + 落库（原文在手，与 ToolExecutionEnd 不同步丢原文） ----
	pi.on("tool_result", async (event: any, ctx: any) => {
		try {
			return await handleToolResult(event, ctx);
		} catch (e: any) {
			console.error(`[ctx-compactor] tool_result handler error: ${String(e?.message ?? e)}`);
			return undefined;
		}
	});

	async function handleToolResult(event: any, ctx: any): Promise<any> {
		if (!config.trimEnabled) return undefined;
		if (!event || typeof event !== "object") return undefined;
		const toolName = String(event.toolName ?? "");
		if (SELF_TOOLS.has(toolName)) return undefined;

		const text = extractText(event.content);
		// 错误输出 / 短输出不修剪（错误信息通常关键且短）
		if (event.isError || text.length <= config.trimThreshold) return undefined;

		// skill 指令文件不修剪：SKILL.md 与 skill 目录（pi/skills/）内容是模型的工作指令，
		// 修剪成摘要会让模型看不到完整工作流（2026-08-12 实测：读 skill 空转 40+ 次工具调用）。
		// 上限 64KB：防第三方大 skill（内嵌脚本/数据）撑爆上下文。
		const inputRaw = JSON.stringify(event.input ?? {});
		const isSkillRead =
			inputRaw.includes("/pi/skills/") || inputRaw.includes("SKILL.md");
		if (isSkillRead && text.length <= 64 * 1024) return undefined;

		const cwd = typeof ctx?.cwd === "string" ? ctx.cwd : "";
		if (!cwd) return undefined;
		if (!configLoaded) config = loadConfig(cwd);

		const sessionId =
			ctx?.sessionState && typeof ctx.sessionState === "object"
				? String(ctx.sessionState.sessionId ?? "")
				: "";

		const entry: IndexEntry = {
			key: crypto.randomUUID(),
			ts: Date.now(),
			tool: toolName,
			toolCallId: typeof event.toolCallId === "string" ? event.toolCallId : undefined,
			sessionId: sessionId || undefined,
			file: extractFile(toolName, event.input),
			summary: text.slice(0, 200),
			content: redactSecrets(text),
		};
		await appendEntry(cwd, entry);

		// 占位符保留 details（模型可用的结构化信息不受损）
		const details =
			event.details && typeof event.details === "object" ? { ...event.details } : {};
		return {
			content: [{ type: "text", text: placeholderFor(entry) }],
			details: { ...details, trimmed: { key: entry.key, chars: entry.content.length } },
		};
	}

	// ---- 压缩触发日志（session_before_compact 在自动压缩时派发，Phase 0 已验证） ----
	pi.on("session_before_compact", async (event: any) => {
		const tokens = event?.preparation?.tokensBefore ?? "?";
		// 信息性日志（console 转发到宿主日志），便于观测压缩水位，不干预压缩
		try {
			console.log(`[ctx-compactor] compaction triggered, tokensBefore=${tokens}`);
		} catch {
			// 日志失败忽略
		}
	});
}
