/**
 * Coding Memory —— BoenMind 编程记忆插件（编程 APP 专用）。
 *
 * 编程会话的项目级记忆：coding_remember 记、coding_recall 检、coding_forget 删。
 * 与全局长期记忆（facts.md，bm-memory）**隔离**——编程记忆按项目分桶存放，
 * 不污染聊天/全局事实（架构 §四·B：应用自己的记忆插件，默认不污染主记忆）。
 *
 * 存储：`~/.boenmind/coding-memory/<项目桶>/facts.jsonl`（JSONL：{ts, fact}）。
 * os.homedir() 由宿主对齐 $BOENMIND_HOME；QuickJS 的 node:fs 写入是 VFS
 * 内存层（不落盘），真实持久化必须走宿主工具 pi.tool("write")——与
 * ctx-compactor 同款约定（全量读-改-写）。
 *
 * 项目桶：显式 project 参数优先；缺省用工具上下文的 cwd（宿主对齐工作目录，
 * 多项目分桶互不串写）。cwd 消毒规则与 ctx-compactor 一致。
 *
 * 边界：
 * - fact ≤ 500 字符（过长截断；记忆是有界字符原则）；
 * - 每项目桶最多 2000 条（超出丢弃最旧）；
 * - remember 同内容去重（已有相同事实不重复写）。
 */

import * as os from "node:os";
import * as path from "node:path";

// 宿主 API 引用（default export 里赋值；pi.tool 持久化调用需要）
let piApi: any;

// ─────────────────────────────── 常量与工具函数 ───────────────────────────────
const MAX_FACT_CHARS = 500;
const MAX_ENTRIES_PER_BUCKET = 2000;

interface Entry {
	ts: number;
	fact: string;
}

/** 记忆根目录（$BOENMIND_HOME/.boenmind/coding-memory/<项目桶>/facts.jsonl）。 */
function memoryRoot(): string {
	return path.join(os.homedir(), ".boenmind", "coding-memory");
}

/** 项目桶：显式 project 优先，否则 cwd 消毒；空 → "default"。 */
function bucketFor(project: string | undefined, cwd: string | undefined): string {
	const raw = (project ?? cwd ?? "").trim();
	if (!raw) return "default";
	return raw.replace(/[\\/:*?"<>|]/g, "_");
}

function bucketDir(bucket: string): string {
	return path.join(memoryRoot(), bucket);
}

function bucketFile(bucket: string): string {
	return path.join(bucketDir(bucket), "facts.jsonl");
}

/** pi.tool("read") 结果 → 文本（content 可能是块数组或字符串）。 */
function blockText(result: any): string {
	if (!result) return "";
	const content = result.content;
	if (typeof content === "string") return content;
	if (Array.isArray(content)) {
		return content
			.filter((b: any) => b && b.type === "text" && typeof b.text === "string")
			.map((b: any) => b.text)
			.join("\n");
	}
	return "";
}

/** 读取项目记忆（JSONL → entries）。文件缺失/损坏行忽略（防污染）。 */
async function loadEntries(bucket: string): Promise<Entry[]> {
	try {
		const read = (await piApi.tool("read", { path: bucketFile(bucket) })) as any;
		const text = blockText(read);
		const out: Entry[] = [];
		for (const line of text.split("\n")) {
			const l = line.trim();
			if (!l) continue;
			try {
				const e = JSON.parse(l) as Entry;
				if (e && typeof e.ts === "number" && typeof e.fact === "string" && e.fact) {
					out.push(e);
				}
			} catch {
				// 非法行丢弃（模型/手改污染不累积）
			}
		}
		return out;
	} catch {
		return [];
	}
}

/** 全量保存（读-改-写；宿主 write 工具真实落盘）。 */
async function saveEntries(bucket: string, entries: Entry[]): Promise<void> {
	const next = entries.map((e) => JSON.stringify(e)).join("\n");
	await piApi.tool("write", { path: bucketFile(bucket), content: next ? next + "\n" : "" });
}

/** 归一化检索词：空白拆分 + 连续 CJK 按 2-gram（中文检索与 ctx-compactor 同思路）。 */
function queryTerms(query: string): string[] {
	const tokens = query
		.split(/\s+/)
		.map((t) => t.trim())
		.filter((t) => t.length > 0);
	const terms: string[] = [];
	for (const t of tokens) {
		if (/[\u4e00-\u9fff]/.test(t)) {
			// 纯中文片段 → 拆 2-gram（"数据库迁移" → 数据/据库/库迁/迁移）
			const chars = Array.from(t);
			for (let i = 0; i + 1 < chars.length; i++) {
				terms.push(chars[i] + chars[i + 1]);
			}
			if (chars.length === 1) terms.push(t);
		} else {
			terms.push(t.toLowerCase());
		}
	}
	return terms;
}

/** 事实是否命中至少一个检索词（大小写不敏感子串；宽松召回，靠评分排序）。 */
function matches(fact: string, terms: string[]): boolean {
	const lower = fact.toLowerCase();
	return terms.some((t) => lower.includes(t));
}

// ─────────────────────────────── 工具执行 ───────────────────────────────
async function runRemember(fact: string | undefined, project: string | undefined, ctx: any): Promise<string> {
	if (!fact || !fact.trim()) return "coding_remember 需要 fact 参数（要记住的事实）";
	let text = fact.trim();
	if (Array.from(text).length > MAX_FACT_CHARS) {
		text = Array.from(text).slice(0, MAX_FACT_CHARS).join("");
	}
	const bucket = bucketFor(project, ctx?.cwd);
	const entries = await loadEntries(bucket);
	if (entries.some((e) => e.fact === text)) {
		return `已存在相同记忆（项目桶 ${bucket}），未重复写入`;
	}
	entries.push({ ts: Date.now(), fact: text });
	// 超上限丢最旧
	while (entries.length > MAX_ENTRIES_PER_BUCKET) entries.shift();
	await saveEntries(bucket, entries);
	return `已记住（项目桶 ${bucket}，共 ${entries.length} 条）`;
}

async function runRecall(query: string | undefined, limit: number | undefined, project: string | undefined, ctx: any): Promise<{ text: string; details: any }> {
	const bucket = bucketFor(project, ctx?.cwd);
	const entries = await loadEntries(bucket);
	if (entries.length === 0) {
		return { text: `项目桶 ${bucket} 还没有记忆。`, details: { bucket, count: 0 } };
	}
	const n = Math.max(1, Math.min(limit ?? 10, 20));
	const q = (query ?? "").trim();
	let picked: Entry[];
	if (!q) {
		// 无查询 → 最近 n 条
		picked = entries.slice(-n).reverse();
	} else {
		const terms = queryTerms(q);
		const scored = entries
			.filter((e) => matches(e.fact, terms))
			.map((e) => ({ e, hits: terms.reduce((acc, t) => acc + (e.fact.toLowerCase().includes(t) ? 1 : 0), 0) }))
			.sort((a, b) => a.e.hits - b.e.hits || a.e.ts - b.e.ts); // 命中多者优先，其次较新
		picked = scored.slice(-n).map((s) => s.e).reverse();
	}
	if (picked.length === 0) {
		return { text: `未找到与「${q}」相关的记忆。`, details: { bucket, count: entries.length, matched: 0 } };
	}
	const lines = picked.map((e) => `- ${e.fact}`);
	return {
		text: `项目桶 ${bucket} 共 ${entries.length} 条记忆，命中 ${picked.length} 条：\n${lines.join("\n")}`,
		details: { bucket, count: entries.length, matched: picked.length },
	};
}

async function runForget(fact: string | undefined, project: string | undefined, ctx: any): Promise<string> {
	if (!fact || !fact.trim()) return "coding_forget 需要 fact 参数（要删除的事实原文）";
	const target = fact.trim();
	const bucket = bucketFor(project, ctx?.cwd);
	const entries = await loadEntries(bucket);
	const kept = entries.filter((e) => e.fact !== target);
	if (kept.length === entries.length) {
		return `未找到与目标相同的事实（项目桶 ${bucket}）`;
	}
	await saveEntries(bucket, kept);
	return `已遗忘 ${entries.length - kept.length} 条（项目桶 ${bucket}，剩 ${kept.length} 条）`;
}

// ─────────────────────────────── 扩展入口 ───────────────────────────────
export default function (pi: any) {
	piApi = pi;

	// startup：观测（每会话一次；确认存储根与插件加载正常）
	pi.on("startup", async (_event: any, ctx: any) => {
		const bucket = bucketFor(undefined, ctx?.cwd);
		const entries = await loadEntries(bucket);
		console.log(`[coding-memory] startup ok, root=${memoryRoot()}, bucket=${bucket}, entries=${entries.length}`);
	});

	pi.registerTool({
		name: "coding_remember",
		label: "coding_remember",
		description:
			"编程记忆：把一条项目事实写入编程记忆库（与全局长期记忆隔离，按项目分桶）。" +
			"适合记录：项目结构结论、用户偏好（命名/风格）、关键决策与理由、踩坑与解法、当前任务进度。" +
			"同内容自动去重；单条上限 500 字符。",
		parameters: {
			type: "object",
			properties: {
				fact: { type: "string", description: "要记住的事实（≤500 字符）" },
				project: { type: "string", description: "可选：项目桶名（缺省用当前工作目录，多项目建议显式指定）" },
			},
			required: ["fact"],
		},

		async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, ctx: any) {
			try {
				const text = await runRemember(params?.fact, params?.project, ctx);
				return { content: [{ type: "text", text }], details: { bucket: bucketFor(params?.project, ctx?.cwd) } };
			} catch (e: any) {
				const message = `[coding-memory] 记忆失败: ${String(e?.message ?? e)}`;
				console.error(message);
				return { content: [{ type: "text", text: message }], details: { error: message } };
			}
		},
	});

	pi.registerTool({
		name: "coding_recall",
		label: "coding_recall",
		description:
			"编程记忆：检索项目记忆。query 为空返回最近若干条；有 query 做关键词匹配" +
			"（中文按相邻两字切词）。编程任务开始或需要项目背景时先检索，避免重复摸索。",
		parameters: {
			type: "object",
			properties: {
				query: { type: "string", description: "可选：检索关键词（如「数据库 迁移」「命名 规范」）" },
				limit: { type: "integer", description: "可选：返回条数 1~20（缺省 10）" },
				project: { type: "string", description: "可选：项目桶名（缺省用当前工作目录）" },
			},
		},

		async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, ctx: any) {
			try {
				const result = await runRecall(params?.query, params?.limit, params?.project, ctx);
				return { content: [{ type: "text", text: result.text }], details: result.details ?? {} };
			} catch (e: any) {
				const message = `[coding-memory] 检索失败: ${String(e?.message ?? e)}`;
				console.error(message);
				return { content: [{ type: "text", text: message }], details: { error: message } };
			}
		},
	});

	pi.registerTool({
		name: "coding_forget",
		label: "coding_forget",
		description: "编程记忆：删除一条记忆（按事实原文精确匹配；先 coding_recall 查到原文再删）。",
		parameters: {
			type: "object",
			properties: {
				fact: { type: "string", description: "要删除的事实原文" },
				project: { type: "string", description: "可选：项目桶名（缺省用当前工作目录）" },
			},
			required: ["fact"],
		},

		async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, ctx: any) {
			try {
				const text = await runForget(params?.fact, params?.project, ctx);
				return { content: [{ type: "text", text }], details: { bucket: bucketFor(params?.project, ctx?.cwd) } };
			} catch (e: any) {
				const message = `[coding-memory] 遗忘失败: ${String(e?.message ?? e)}`;
				console.error(message);
				return { content: [{ type: "text", text: message }], details: { error: message } };
			}
		},
	});
}
