/**
 * Web Search —— BoenMind 多源搜索增强插件。
 *
 * 自研实现，吸收 Hermes web-multisearch 聚合思路（并行扇出 / URL 规范化去重 /
 * 多源交叉验证标注 / 单源失败隔离），不复制其代码。在其之上补强：
 *   ① 免费源优先 + 用量追踪（~/.boenmind/web-search/quota.json），额度耗尽自动
 *      切换，按「剩余额度比例 + 今日调用次数」加权选源，平均使用不烧单源；
 *   ② 同查询结果缓存（项目级 JSONL，TTL 可配），避免重复消耗免费配额；
 *   ③ 429/5xx 单源指数退避重试，替代直接丢弃；
 *   ④ web_fetch 单页正文提取（jina Reader，截断摘要，SSRF 防护），补齐
 *      「只搜不读」的短板。
 *
 * 沙箱事实（Phase 0 调研结论）：
 * - 网络只能走宿主 hostcall `pi.http({url, method, headers, body, timeout})`，
 *   仅支持 GET/POST，TLS 强制；exec 被插件政策拒绝，不能用 bash/curl；
 * - node:fs 写是 VFS 虚拟层（不落盘，重启丢失），真实持久化必须
 *   `pi.tool("write")`；读有 host 回退，可直接 node:fs；
 * - node:os homedir() 可用（取 $HOME）→ 用户级文件放 ~/.boenmind 下。
 *
 * 配置：插件目录内 settings.json（~/.boenmind/extensions/web-search/settings.json，
 * 由设置页经后端 bm-server 持久化；插件启动时读取，与 extension.json 的 settings 声明对齐）。
 * 用量：<cwd>/.boenmind/web-search/quota.json（本插件自行读写；沙箱 pi.tool("write")
 * 限制在 workspace 内，故按项目记录）。
 * 缓存：<cwd>/.boenmind/web-search-cache.jsonl。
 */
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import * as crypto from "node:crypto";

// ─────────────────────────────── 常量 ───────────────────────────────

/** 每源请求超时（毫秒）——收紧：免费源在部分网络环境可能长时间无响应 */
const SOURCE_TIMEOUT_MS = 8_000;
/** 整次搜索的兜底超时（毫秒），用 Promise.race 真正收口 */
const GLOBAL_TIMEOUT_MS = 20_000;
/** 429/5xx 重试次数上限（指数退避 500ms/1000ms） */
const MAX_RETRIES = 1;
/** 源失败后的惩罚窗口（毫秒）：窗口内选源时跳过该源 */
const FAIL_PENALTY_MS = 5 * 60_000;
/** 429 标记耗尽后的冷却期（毫秒）：期间跳过该源；冷却过期自动复活重试。
 *  tokens 源（Jina）无月度重置，一次临时限流不能永久禁用；按次源在冷却
 *  过期后也会再试一次（真实额度以服务端为准，429 探测兜底）。 */
const EXHAUST_COOLDOWN_MS = 60 * 60_000;
/** 输出单条摘要最大字符数（控制进上下文的体积） */
const DESC_MAX_CHARS = 300;
/** web_fetch 正文返回上限（字符） */
const FETCH_MAX_CHARS = 8_000;
/** quick 档并行源数；deep 档上限 */
const QUICK_SOURCES = 2;
const DEEP_SOURCES_MAX = 4;
/** 每源多取的倍数（合并去重后才够截断） */
const PER_SOURCE_MULT = 2;
/** 缓存轮转阈值（字节） */
const CACHE_MAX_BYTES = 8 * 1024 * 1024;

/** 各源元信息：档位（quick 只跑 free）+ 免费额度声明 */
const SOURCE_META = {
	jina: {
		displayName: "Jina",
		tier: "free" as const,
		quota: { total: 10_000_000, unit: "tokens" },
	},
	tavily: {
		displayName: "Tavily",
		tier: "free" as const,
		quota: { total: 1000, unit: "calls", reset: "monthly" },
	},
	exa: {
		displayName: "Exa",
		tier: "free" as const,
		quota: { total: 1000, unit: "calls", reset: "monthly" },
	},
	serper: {
		displayName: "Serper",
		tier: "paid" as const,
		quota: { total: 2500, unit: "calls" },
	},
} as const;

/** 网页提取源元信息（不参与搜索选源，仅 web_fetch 用） */
const EXTRACT_META = {
	firecrawl: {
		displayName: "Firecrawl",
		quota: { total: 500, unit: "calls", reset: "monthly" },
	},
} as const;

/** URL 规范化去重时丢弃的常见跟踪参数（跨源同一页面只算一条） */
const TRACKING_PARAMS = new Set([
	"utm_source", "utm_medium", "utm_campaign", "utm_term", "utm_content",
	"fbclid", "gclid", "yclid", "igshid", "mc_cid", "mc_eid",
	"ref", "ref_src", "source", "spm", "wfr", "_ga", "from",
]);

// ─────────────────────────────── 类型与默认配置 ───────────────────────────────

/** 自定义源（用户自接任意 JSON 搜索 API，如自建 SearXNG） */
interface CustomSource {
	enabled: boolean;
	name: string;
	url: string; // 请求 URL 模板，{query} 占位
	apiKeyHeader: string;
	apiKey: string;
	resultsPath: string; // JSON 点分路径 → 结果数组
	titlePath: string;
	urlPath: string;
	descPath: string;
}

interface Settings {
	mode: "quick" | "deep";
	cacheTtlSeconds: number;
	sources: Record<string, { enabled: boolean; apiKey: string }>;
	/** 自定义源（custom1 / custom2，设置页平铺字段） */
	custom: CustomSource[];
}

function defaultCustomSource(): CustomSource {
	return {
		enabled: false,
		name: "",
		url: "",
		apiKeyHeader: "",
		apiKey: "",
		resultsPath: "results",
		titlePath: "title",
		urlPath: "url",
		descPath: "description",
	};
}

const DEFAULTS: Settings = {
	mode: "quick",
	cacheTtlSeconds: 600,
	sources: {
		jina: { enabled: true, apiKey: "" },
		tavily: { enabled: true, apiKey: "" },
		exa: { enabled: true, apiKey: "" },
		firecrawl: { enabled: true, apiKey: "" },
		serper: { enabled: false, apiKey: "" },
	},
	custom: [defaultCustomSource(), defaultCustomSource()],
};

interface QuotaState {
	used: number;
	total: number;
	unit: string;
	reset?: string;
	exhaustedAt?: number;
	callsToday: number;
	today: string;
	lastError?: string;
	lastErrorAt?: number;
}

interface SearchItem {
	title: string;
	url: string;
	description: string;
	position: number;
}

interface SourceOutcome {
	source: string;
	items: SearchItem[];
	ms: number;
}

interface HttpOutcome {
	ok: boolean;
	status?: number;
	headers: Record<string, string>;
	body: string;
	error?: string;
	timeout?: boolean;
}

// 宿主 API 引用（default export 里赋值）
let piApi: ExtensionAPI | undefined;
// 运行时配置（每次执行前重新读文件，保证设置页改动即时生效）
let settings: Settings = { ...DEFAULTS, sources: deepClone(DEFAULTS.sources) };
let settingsLoaded = false;
// 用量状态（内存镜像，读取后操作，结束时落盘）
let quota: Record<string, QuotaState> = {};

function deepClone<T>(v: T): T {
	return JSON.parse(JSON.stringify(v)) as T;
}

// ─────────────────────────────── 路径与持久化 ───────────────────────────────

/**
 * 插件自身目录：沙箱 node:fs 读被限制在 workspace 与扩展根内，
 * 用户级目录（~/.boenmind/plugin-settings）读不到 → 设置文件放插件目录内
 * （~/.boenmind/extensions/web-search/settings.json，由后端 bm-server 写入）。
 * 定位方式：import.meta.url（扩展入口 index.ts）推导目录，失败时回退
 * ~/.boenmind/extensions/web-search。
 */
function pluginDir(): string {
	try {
		const meta = import.meta as { url?: string } | undefined;
		if (meta?.url && meta.url.startsWith("file://")) {
			const p = decodeURIComponent(meta.url.slice("file://".length).split(/[?#]/)[0]);
			return path.dirname(p);
		}
	} catch {
		// import.meta.url 不可用时走回退
	}
	const fallback = path.join(os.homedir(), ".boenmind", "extensions", "web-search");
	return fallback;
}

function settingsPath(): string {
	return path.join(pluginDir(), "settings.json");
}

/** 缓存与用量仍放用户级/项目级目录（仅插件自身写入，沙箱内不读）。 */
function userDir(): string {
	try {
		const home = os.homedir();
		if (home) return path.join(home, ".boenmind");
	} catch {
		// 拿不到 home 时降级：用当前工作目录下的 .boenmind（项目级）
	}
	return ".boenmind";
}

function quotaPath(cwd: string): string {
	// 沙箱 pi.tool("write") 被限制在 workspace（cwd）内，用户级目录写不进去；
	// 用量按项目记录（跨项目不累计，真实额度以服务端为准，429 探测兜底）。
	return path.join(cwd, ".boenmind", "web-search", "quota.json");
}

function cachePath(cwd: string): string {
	return path.join(cwd, ".boenmind", "web-search-cache.jsonl");
}

/** 读文件：node:fs 有 host 回退，可直接用；失败返回 null。 */
function readFileSafe(file: string): string | null {
	try {
		return fs.readFileSync(file, "utf8");
	} catch {
		return null;
	}
}

/** 写文件：node:fs 写是 VFS 虚拟层不落盘，必须走宿主 pi.tool("write")。 */
async function writeFileReal(file: string, content: string): Promise<void> {
	await (piApi as any).tool("write", { path: file, content });
}

// ─────────────────────────────── 配置与用量加载 ───────────────────────────────

/** 从设置页写入的配置读取设置（缺字段用默认值）。存储为扁平 {key: value}。 */
function loadSettings(): Settings {
	const out: Settings = {
		...DEFAULTS,
		sources: deepClone(DEFAULTS.sources),
		custom: [defaultCustomSource(), defaultCustomSource()],
	};
	try {
		const text = readFileSafe(settingsPath());
		if (!text) return out;
		const raw = JSON.parse(text) as Record<string, unknown>;
		if (raw.mode === "deep" || raw.mode === "quick") out.mode = raw.mode;
		const ttl = Number(raw.cacheTtlSeconds);
		if (Number.isFinite(ttl) && ttl >= 0) out.cacheTtlSeconds = Math.min(Math.floor(ttl), 86400);
		// 扁平 key：sources.<name>.enabled / sources.<name>.apiKey
		for (const key of Object.keys(out.sources)) {
			const enabled = raw[`sources.${key}.enabled`];
			if (typeof enabled === "boolean") out.sources[key].enabled = enabled;
			const apiKey = raw[`sources.${key}.apiKey`];
			if (typeof apiKey === "string") out.sources[key].apiKey = apiKey.trim();
		}
		// 自定义源：custom1.* / custom2.* 平铺字段
		for (let i = 0; i < out.custom.length; i++) {
			const prefix = `custom${i + 1}`;
			const c = out.custom[i];
			const str = (k: string): string => {
				const v = raw[`${prefix}.${k}`];
				return typeof v === "string" ? v.trim() : c[k as keyof CustomSource] as string;
			};
			const bool = (k: string): boolean => {
				const v = raw[`${prefix}.${k}`];
				return typeof v === "boolean" ? v : (c[k as keyof CustomSource] as boolean);
			};
			c.enabled = bool("enabled");
			c.name = str("name");
			c.url = str("url");
			c.apiKeyHeader = str("apiKeyHeader");
			c.apiKey = str("apiKey");
			c.resultsPath = str("resultsPath") || "results";
			c.titlePath = str("titlePath") || "title";
			c.urlPath = str("urlPath") || "url";
			c.descPath = str("descPath") || "description";
		}
	} catch (e: any) {
		console.error(`[web-search] loadSettings failed: ${String(e?.message ?? e)}`);
	}
	return out;
}

/** 读用量文件；文件不存在时按 SOURCE_META / EXTRACT_META 声明额度初始化。 */
function loadQuota(cwd: string): Record<string, QuotaState> {
	const out: Record<string, QuotaState> = {};
	for (const [key, meta] of Object.entries({ ...SOURCE_META, ...EXTRACT_META })) {
		out[key] = {
			used: 0,
			total: meta.quota.total,
			unit: meta.quota.unit,
			reset: meta.quota.reset,
			callsToday: 0,
			today: todayStr(),
		};
	}
	try {
		const text = readFileSafe(quotaPath(cwd));
		if (text) {
			const raw = JSON.parse(text) as Record<string, Partial<QuotaState>>;
			for (const key of Object.keys(out)) {
				const r = raw[key];
				if (!r || typeof r !== "object") continue;
				out[key] = { ...out[key], ...r, total: out[key].total };
				// 跨天重置"今日调用"计数
				if (r.today !== todayStr()) {
					out[key].callsToday = 0;
					out[key].today = todayStr();
				}
			}
		}
	} catch {
		// 用量文件损坏 → 用默认声明额度
	}
	return out;
}

async function saveQuota(cwd: string): Promise<void> {
	try {
		await writeFileReal(quotaPath(cwd), JSON.stringify(quota, null, 2));
	} catch (e: any) {
		console.error(`[web-search] saveQuota failed: ${String(e?.message ?? e)}`);
	}
}

function todayStr(): string {
	return new Date().toISOString().slice(0, 10);
}

/** 源是否已耗尽（含月度重置与冷却复活）。 */
function isExhausted(key: string): boolean {
	const q = quota[key];
	if (!q) return false;
	if (q.exhaustedAt) {
		const now = Date.now();
		// 月度重置的源：跨月自动复活
		const monthlyReset =
			q.reset === "monthly" && new Date(q.exhaustedAt).toISOString().slice(0, 7) !== todayStr().slice(0, 7);
		if (monthlyReset) {
			q.exhaustedAt = undefined;
			q.used = 0;
		} else if (now - q.exhaustedAt < EXHAUST_COOLDOWN_MS) {
			return true; // 冷却期内视为耗尽
		} else {
			// 冷却过期自动复活（used 保留，继续探测真实额度）
			q.exhaustedAt = undefined;
		}
	}
	// 按次计费的源（calls 单位）可用 used >= total 判定
	if (q.unit === "calls" && q.used >= q.total) return true;
	return false;
}

/** 源剩余额度比例（0~1）；未知额度（自定义源）默认视作充足。 */
function remainingRatio(key: string): number {
	const q = quota[key];
	if (!q || q.total <= 0) return 1;
	if (q.unit === "calls") return Math.max(0, 1 - q.used / q.total);
	return 1; // tokens 类额度客户端无法精确计数，靠 429/响应头探测
}

/**
 * 选择本次搜索的源集合：
 * ① 配置启用且未耗尽的源（含自定义源，视为 free 档）；
 * ② 按档位过滤（quick 只留 free）；
 * ③ 排除最近 FAIL_PENALTY_MS 内失败过的源（网络不稳的源自动让位，除非无可用源）；
 * ④ 按「剩余额度比例 × 100 − 今日调用 × 3」加权排序（比例优先、平均使用）；
 * ⑤ 取前 N 个并行（quick=2，deep≤4）。
 */
function selectSources(mode: "quick" | "deep"): string[] {
	const enabled: string[] = [];
	for (const key of Object.keys(SOURCE_META)) {
		const s = settings.sources[key];
		if (s?.enabled && Boolean(s.apiKey) && !isExhausted(key)) enabled.push(key);
	}
	for (const c of settings.custom) {
		if (c.enabled && c.name && c.url && !isExhausted(`custom:${c.name}`)) {
			enabled.push(`custom:${c.name}`);
		}
	}
	const scoped =
		mode === "deep"
			? enabled
			: enabled.filter((k) => k.startsWith("custom:") || SOURCE_META[k].tier === "free");
	const now = Date.now();
	// 失败惩罚：最近失败过的源降分 1000（有可用源时直接让位）
	const scored = scoped
		.map((k) => {
			const q = quota[k];
			const penalty = q?.lastErrorAt && now - q.lastErrorAt < FAIL_PENALTY_MS ? 1000 : 0;
			return { key: k, score: remainingRatio(k) * 100 - (q?.callsToday ?? 0) * 3 - penalty };
		})
		.sort((a, b) => b.score - a.score);
	const usable = scored.filter((s) => s.score > -900);
	const pick = usable.length > 0 ? usable : scored; // 全部在惩罚窗口内时仍选（尽力而为）
	const limit = mode === "deep" ? DEEP_SOURCES_MAX : QUICK_SOURCES;
	return pick.slice(0, limit).map((s) => s.key);
}

// ─────────────────────────────── HTTP 封装 ───────────────────────────────

/** 宿主 http hostcall 包装：错误归一为 HttpOutcome，绝不抛异常。 */
async function httpRequest(req: {
	url: string;
	method?: "GET" | "POST";
	headers?: Record<string, string>;
	body?: string;
	timeoutMs?: number;
}): Promise<HttpOutcome> {
	try {
		const res = await (piApi as any).http({
			url: req.url,
			method: req.method ?? "GET",
			headers: req.headers ?? {},
			...(req.body !== undefined ? { body: req.body } : {}),
			timeout: req.timeoutMs ?? SOURCE_TIMEOUT_MS,
		});
		const status = Number(res?.status) || 0;
		return {
			ok: status >= 200 && status < 300,
			status,
			headers: res?.headers && typeof res.headers === "object" ? res.headers : {},
			body: typeof res?.body === "string" ? res.body : "",
		};
	} catch (e: any) {
		const msg = String(e?.message ?? e ?? "unknown error");
		return {
			ok: false,
			headers: {},
			body: "",
			error: msg,
			timeout: /timeout|timed out/i.test(msg),
		};
	}
}

/** 带退避的重试：429/5xx/超时最多重试 MAX_RETRIES 次。 */
async function httpRequestRetry(req: Parameters<typeof httpRequest>[0]): Promise<HttpOutcome> {
	let delay = 500;
	let last = await httpRequest(req);
	for (let i = 0; i < MAX_RETRIES && shouldRetry(last); i++) {
		await sleep(delay);
		delay *= 2;
		last = await httpRequest(req);
	}
	return last;

	function shouldRetry(r: HttpOutcome): boolean {
		if (!r.ok) {
			if (r.status === 429 || (r.status >= 500 && r.status < 600)) return true;
			if (r.timeout) return true;
		}
		return false;
	}
}

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

// ─────────────────────────────── URL 规范化与去重 ───────────────────────────────

/** 规范化 URL 用于去重：小写 host、去 www、去尾斜杠、丢弃跟踪参数、query 排序。 */
function normalizeUrl(raw: string): string {
	const url = (raw ?? "").trim();
	if (!url) return "";
	const m = /^([a-z][a-z0-9+.-]*:\/\/)?([^/?#]*)([^?#]*)(\?[^#]*)?(#.*)?$/i.exec(url);
	if (!m) return url;
	const hostRaw = m[2] ?? "";
	let host = hostRaw.toLowerCase();
	if (host.startsWith("www.")) host = host.slice(4);
	const rawPath = m[3] ?? "";
	const pathPart = rawPath ? rawPath.replace(/\/+$/, "") || "/" : "";
	const queryRaw = m[4] ? m[4].slice(1) : "";
	let query = "";
	if (queryRaw) {
		const keep: Array<[string, string]> = [];
		for (const pair of queryRaw.split("&")) {
			const eq = pair.indexOf("=");
			const k = eq >= 0 ? pair.slice(0, eq) : pair;
			const v = eq >= 0 ? pair.slice(eq + 1) : "";
			if (k && !TRACKING_PARAMS.has(k.toLowerCase())) keep.push([k, v]);
		}
		keep.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
		if (keep.length > 0) query = "?" + keep.map(([k, v]) => `${k}=${v}`).join("&");
	}
	return `${host}${pathPart}${query}`;
}

/** 标题 shingle 相似度（0~1）：用于合并转载/聚合站重复内容。 */
function titleSimilarity(a: string, b: string): number {
	const ta = norm(a);
	const tb = norm(b);
	if (!ta || !tb) return 0;
	const gramsA = shingles(ta);
	const gramsB = shingles(tb);
	if (gramsA.size === 0 || gramsB.size === 0) return 0;
	let inter = 0;
	for (const g of gramsA) if (gramsB.has(g)) inter++;
	return inter / Math.min(gramsA.size, gramsB.size);
}

function norm(s: string): string {
	return s.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, " ").trim();
}

/**
 * 标题 shingle 化：中文按字符 bigram（无空格分词），英文按词对 bigram。
 * 用交并比算相似度，识别转载/聚合站标题变体。
 */
function shingles(s: string): Set<string> {
	const words = s.split(" ").filter((w) => w.length > 1);
	const out = new Set<string>();
	const cjkRe = /[\u4e00-\u9fff]/;
	const enWords: string[] = [];
	for (const w of words) {
		if (cjkRe.test(w) && w.length >= 2) {
			for (let i = 0; i + 1 < w.length; i++) out.add(`c:${w.slice(i, i + 2)}`);
		} else {
			enWords.push(w);
		}
	}
	for (let i = 0; i + 1 < enWords.length; i++) out.add(`${enWords[i]} ${enWords[i + 1]}`);
	return out;
}

// ─────────────────────────────── 源适配器 ───────────────────────────────

/** Jina Search（s.jina.ai）：返回 LLM 友好的 markdown，解析其中的链接结果。 */
async function jinaSearch(query: string, limit: number): Promise<HttpOutcome> {
	return httpRequestRetry({
		url: `https://s.jina.ai/?q=${encodeURIComponent(query)}`,
		headers: {
			Authorization: `Bearer ${settings.sources.jina.apiKey}`,
			Accept: "text/markdown",
		},
	});
}

/** 从 Jina Search 的 markdown 里提取结果（标题/URL/描述，按出现顺序即排名）。 */
function parseJinaMarkdown(text: string, limit: number): SearchItem[] {
	const results: SearchItem[] = [];
	const lines = text.split(/\r?\n/).map((l) => l.trim());
	const linkRe = /\[([^\]]{1,300})\]\(([^)]+)\)/;
	let i = 0;
	while (i < lines.length && results.length < limit) {
		const line = lines[i];
		if (!line) {
			i++;
			continue;
		}
		let title = "";
		let url = "";
		const link = linkRe.exec(line);
		if (link) {
			title = link[1].trim();
			url = link[2].trim();
		} else if (line.startsWith("###")) {
			title = line.replace(/^#+\s*/, "").trim();
			// 下一个非空行通常是裸 URL
			let j = i + 1;
			while (j < lines.length && !lines[j]) j++;
			if (j < lines.length && !linkRe.test(lines[j]) && !lines[j].startsWith("#")) {
				url = lines[j];
				i = j;
			}
		} else {
			i++;
			continue;
		}
		// 收集后续描述行，直到下一个结果标记
		const descParts: string[] = [];
		let j = i + 1;
		while (j < lines.length && lines[j] && !linkRe.test(lines[j]) && !lines[j].startsWith("#")) {
			descParts.push(lines[j]);
			j++;
		}
		i = j;
		if (!url) continue;
		results.push({
			title: title || url,
			url,
			description: descParts.join(" ").slice(0, 500),
			position: results.length + 1,
		});
	}
	return results;
}

/** Tavily（api.tavily.com/search）：JSON 结果。 */
async function tavilySearch(query: string, limit: number): Promise<HttpOutcome> {
	return httpRequestRetry({
		url: "https://api.tavily.com/search",
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify({
			api_key: settings.sources.tavily.apiKey,
			query,
			max_results: limit,
			search_depth: "basic",
		}),
	});
}

function parseTavily(body: string): SearchItem[] {
	try {
		const data = JSON.parse(body) as {
			results?: Array<{ title?: string; url?: string; content?: string }>;
		};
		return (data.results ?? []).map((r, idx) => ({
			title: String(r.title ?? ""),
			url: String(r.url ?? ""),
			description: String(r.content ?? "").slice(0, 500),
			position: idx + 1,
		}));
	} catch {
		return [];
	}
}

/** Exa（api.exa.ai/search）：语义搜索，JSON 结果。 */
async function exaSearch(query: string, limit: number): Promise<HttpOutcome> {
	return httpRequestRetry({
		url: "https://api.exa.ai/search",
		method: "POST",
		headers: {
			"x-api-key": settings.sources.exa.apiKey,
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ query, numResults: Math.min(limit, 100) }),
	});
}

function parseExa(body: string): SearchItem[] {
	try {
		const data = JSON.parse(body) as {
			results?: Array<{ title?: string; url?: string; text?: string; publishedDate?: string }>;
		};
		return (data.results ?? []).map((r, idx) => {
			const date = r.publishedDate ? `（${r.publishedDate.slice(0, 10)}）` : "";
			return {
				title: `${String(r.title ?? "")}${date}`,
				url: String(r.url ?? ""),
				description: String(r.text ?? "").slice(0, 500),
				position: idx + 1,
			};
		});
	} catch {
		return [];
	}
}

/** JSON 点分路径取值（如 "data.list" → obj.data.list）。 */
function resolvePath(obj: unknown, dotted: string): unknown {
	let cur: unknown = obj;
	for (const part of dotted.split(".")) {
		if (cur == null || typeof cur !== "object") return undefined;
		cur = (cur as Record<string, unknown>)[part];
	}
	return cur;
}

/** 自定义源：URL 模板 {query} 替换 + 可选认证头。 */
async function customSearch(key: string, query: string): Promise<HttpOutcome> {
	const c = settings.custom.find((c) => `custom:${c.name}` === key);
	if (!c) {
		return { ok: false, status: 0, headers: {}, body: "", error: "custom source not found" };
	}
	const url = c.url.replace(/\{query\}/g, encodeURIComponent(query));
	const headers: Record<string, string> = {};
	if (c.apiKeyHeader && c.apiKey) headers[c.apiKeyHeader] = c.apiKey;
	return httpRequestRetry({ url, method: "GET", headers });
}

/** 按配置的字段路径解析自定义源 JSON 结果。 */
function parseCustom(body: string, src: CustomSource): SearchItem[] {
	try {
		const data = JSON.parse(body) as unknown;
		const arr = resolvePath(data, src.resultsPath);
		if (!Array.isArray(arr)) return [];
		const items: SearchItem[] = [];
		for (const el of arr) {
			if (el == null || typeof el !== "object") continue;
			const rec = el as Record<string, unknown>;
			const title = String(resolvePath(rec, src.titlePath) ?? "");
			const url = String(resolvePath(rec, src.urlPath) ?? "");
			if (!url) continue;
			items.push({
				title: title || url,
				url,
				description: String(resolvePath(rec, src.descPath) ?? "").slice(0, 500),
				position: items.length + 1,
			});
		}
		return items;
	} catch {
		return [];
	}
}

/** Serper（google.serper.dev/search）：Google SERP。 */
async function serperSearch(query: string, limit: number): Promise<HttpOutcome> {
	return httpRequestRetry({
		url: "https://google.serper.dev/search",
		method: "POST",
		headers: {
			"X-API-KEY": settings.sources.serper.apiKey,
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ q: query, num: Math.min(limit, 100) }),
	});
}

function parseSerper(body: string): SearchItem[] {
	try {
		const data = JSON.parse(body) as { organic?: Array<{ title?: string; link?: string; snippet?: string }> };
		return (data.organic ?? []).map((r, idx) => ({
			title: String(r.title ?? ""),
			url: String(r.link ?? ""),
			description: String(r.snippet ?? "").slice(0, 500),
			position: idx + 1,
		}));
	} catch {
		return [];
	}
}

/** 统一的源搜索入口：调用 + 用量记录 + 探测额度耗尽。 */
async function searchSource(key: string, query: string, limit: number): Promise<SourceOutcome> {
	const started = Date.now();
	let outcome: HttpOutcome;
	if (key === "jina") outcome = await jinaSearch(query, limit);
	else if (key === "tavily") outcome = await tavilySearch(query, limit);
	else if (key === "exa") outcome = await exaSearch(query, limit);
	else if (key === "serper") outcome = await serperSearch(query, limit);
	else if (key.startsWith("custom:")) outcome = await customSearch(key, query);
	else return { source: key, items: [], ms: Date.now() - started };

	// 用量记录：静态源按声明初始化，自定义源动态创建（未知额度，429 探测耗尽）
	if (!quota[key]) {
		const meta = SOURCE_META[key as keyof typeof SOURCE_META];
		quota[key] = meta
			? {
					used: 0,
					total: meta.quota.total,
					unit: meta.quota.unit,
					reset: meta.quota.reset,
					callsToday: 0,
					today: todayStr(),
				}
			: { used: 0, total: 0, unit: "unknown", callsToday: 0, today: todayStr() };
	}
	const q = quota[key];
	if (q) {
		q.callsToday = (q.callsToday ?? 0) + 1;
		if (q.unit === "calls" && q.total > 0 && q.used < q.total) q.used += 1; // 按次计费的源精确计数
	}

	if (!outcome.ok) {
		// 429 或额度类错误 → 标记耗尽；其他错误只记录
		const exhausted =
			outcome.status === 429 ||
			/quota|limit|credit|insufficient|exhaust/i.test(outcome.error ?? "") ||
			/quota|limit|credit|insufficient|exhaust/i.test(outcome.body.slice(0, 500));
		if (q) {
			q.lastError = outcome.error ?? `HTTP ${outcome.status ?? "?"}`;
			q.lastErrorAt = Date.now();
			if (exhausted) {
				q.exhaustedAt = Date.now();
				q.used = q.total;
			}
		}
		return { source: key, items: [], ms: Date.now() - started };
	}

	// 响应头里的限流剩余量（部分源提供）→ 更新声明额度外的探测信息
	const remainHeader = findHeader(outcome.headers, "x-ratelimit-remaining");
	if (q && remainHeader) {
		const remain = Number(remainHeader);
		const limitHeader = findHeader(outcome.headers, "x-ratelimit-limit");
		if (Number.isFinite(remain) && remain >= 0) {
			const total = Number(limitHeader);
			if (Number.isFinite(total) && total > 0) q.total = total;
			q.used = Math.max(0, q.total - remain);
			if (remain <= 0) q.exhaustedAt = Date.now();
		}
	}

	let items: SearchItem[] = [];
	if (key === "jina") items = parseJinaMarkdown(outcome.body, limit);
	else if (key === "tavily") items = parseTavily(outcome.body);
	else if (key === "exa") items = parseExa(outcome.body);
	else if (key === "serper") items = parseSerper(outcome.body);
	else if (key.startsWith("custom:")) {
		const c = settings.custom.find((c) => `custom:${c.name}` === key);
		if (c) items = parseCustom(outcome.body, c);
	}
	return { source: key, items, ms: Date.now() - started };
}

function findHeader(headers: Record<string, string>, name: string): string | undefined {
	const lower = name.toLowerCase();
	for (const [k, v] of Object.entries(headers)) {
		if (k.toLowerCase() === lower) return v;
	}
	return undefined;
}

// ─────────────────────────────── 合并与标注 ───────────────────────────────

interface MergedItem {
	url: string;
	title: string;
	description: string;
	sources: Set<string>;
	minPos: number;
}

/**
 * 合并各源结果：
 * ① 按规范化 URL 去重（同页多源只算一条）；
 * ② 标题相似度高的（转载/聚合站）合并；
 * ③ 统计来源集合与最靠前位置。
 */
function mergeResults(perSource: SourceOutcome[]): MergedItem[] {
	const merged: MergedItem[] = [];
	const byUrl = new Map<string, MergedItem>();

	const add = (item: SearchItem, src: string) => {
		if (!item.url) return;
		const key = normalizeUrl(item.url);
		let m = byUrl.get(key);
		if (!m) {
			// 无 URL 命中时尝试标题相似合并（转载页 URL 不同）
			m = merged.find((x) => titleSimilarity(x.title, item.title) > 0.7);
			if (m) {
				byUrl.set(key, m);
			} else {
				m = { url: item.url, title: item.title, description: item.description, sources: new Set(), minPos: Number.MAX_SAFE_INTEGER };
				merged.push(m);
				byUrl.set(key, m);
			}
		}
		m.sources.add(src);
		if (item.position > 0) m.minPos = Math.min(m.minPos, item.position);
		// 保留更完整的 title/description
		if (item.title.length > m.title.length) m.title = item.title;
		if (item.description.length > m.description.length) m.description = item.description;
	};

	for (const so of perSource) {
		for (const item of so.items) add(item, so.source);
	}
	return merged;
}

/** 排序（多源确认优先，其次最靠前位置）并输出带来源标注的最终列表。 */
function finalize(items: MergedItem[], limit: number): Array<{ title: string; url: string; description: string; position: number }> {
	const sorted = [...items].sort((a, b) => {
		if (a.sources.size !== b.sources.size) return b.sources.size - a.sources.size;
		return a.minPos - b.minPos;
	});
	return sorted.slice(0, limit).map((m, idx) => {
		const srcs = [...m.sources].sort().join("|");
		const prefix = `[${srcs}] `;
		let description = m.description;
		let title = m.title;
		if (description) description = prefix + description.slice(0, DESC_MAX_CHARS);
		else if (title) title = prefix + title;
		else title = prefix.trim();
		return { title, url: m.url, description, position: idx + 1 };
	});
}

// ─────────────────────────────── 结果缓存 ───────────────────────────────

interface CacheEntry {
	key: string;
	ts: number;
	query: string;
	mode: string;
	limit: number;
	items: Array<{ title: string; url: string; description: string; position: number }>;
}

/** 缓存键 = 查询 + 档位 + 返回条数（limit 不同不共享缓存，避免大 limit 拿到小结果） */
function cacheKey(query: string, mode: string, limit: number): string {
	return crypto
		.createHash("sha1")
		.update(`${query}\u0000${mode}\u0000${limit}`)
		.digest("hex")
		.slice(0, 16);
}

/** 读缓存：node:fs 读有 host 回退；JSONL 逐行校验防污染。 */
function cacheGet(cwd: string, query: string, mode: string, limit: number): CacheEntry | null {
	const ttl = settings.cacheTtlSeconds;
	if (!ttl || ttl <= 0) return null;
	const text = readFileSafe(cachePath(cwd));
	if (!text) return null;
	const want = cacheKey(query, mode, limit);
	const now = Date.now();
	for (const line of text.split("\n")) {
		if (!line.trim()) continue;
		try {
			const entry = JSON.parse(line) as CacheEntry;
			if (entry.key === want && entry.query === query && entry.mode === mode && entry.limit === limit) {
				if (now - entry.ts <= ttl * 1000) return entry;
				return null;
			}
		} catch {
			// 跳过损坏行
		}
	}
	return null;
}

/** 写缓存：真实落盘走 pi.tool("write")；全量读-改-写 + 超限丢最旧一半。 */
async function cachePut(cwd: string, query: string, mode: string, limit: number, items: CacheEntry["items"]): Promise<void> {
	try {
		const file = cachePath(cwd);
		let existing = readFileSafe(file) ?? "";
		const validLines = existing
			.split("\n")
			.filter((l) => l.trim() !== "")
			.filter((l) => {
				try {
					JSON.parse(l);
					return true;
				} catch {
					return false;
				}
			});
		let kept = validLines.join("\n");
		if (kept.length > CACHE_MAX_BYTES) kept = kept.slice(kept.length / 2);
		const entry: CacheEntry = {
			key: cacheKey(query, mode, limit),
			ts: Date.now(),
			query,
			mode,
			limit,
			items,
		};
		const next = kept.length > 0 ? kept + "\n" : "";
		await writeFileReal(file, next + JSON.stringify(entry) + "\n");
	} catch (e: any) {
		console.error(`[web-search] cachePut failed: ${String(e?.message ?? e)}`);
	}
}

// ─────────────────────────────── web_fetch 缓存 ───────────────────────────────

interface FetchCacheEntry {
	key: string;
	ts: number;
	url: string;
	title: string;
	body: string;
	source: string;
}

function fetchCachePath(cwd: string): string {
	return path.join(cwd, ".boenmind", "web-fetch-cache.jsonl");
}

/** 读取缓存：相同 URL 在 TTL 内直接返回，避免重复抓取重复烧额度。 */
function fetchCacheGet(cwd: string, url: string): FetchCacheEntry | null {
	const ttl = settings.cacheTtlSeconds;
	if (!ttl || ttl <= 0) return null;
	const text = readFileSafe(fetchCachePath(cwd));
	if (!text) return null;
	const want = crypto.createHash("sha1").update(url).digest("hex").slice(0, 16);
	const now = Date.now();
	for (const line of text.split("\n")) {
		if (!line.trim()) continue;
		try {
			const entry = JSON.parse(line) as FetchCacheEntry;
			if (entry.key === want && entry.url === url) {
				if (now - entry.ts <= ttl * 1000) return entry;
				return null;
			}
		} catch {
			// 跳过损坏行
		}
	}
	return null;
}

async function fetchCachePut(cwd: string, url: string, title: string, body: string, source: string): Promise<void> {
	try {
		const file = fetchCachePath(cwd);
		let existing = readFileSafe(file) ?? "";
		const validLines = existing
			.split("\n")
			.filter((l) => l.trim() !== "")
			.filter((l) => {
				try {
					JSON.parse(l);
					return true;
				} catch {
					return false;
				}
			});
		let kept = validLines.join("\n");
		if (kept.length > CACHE_MAX_BYTES) kept = kept.slice(kept.length / 2);
		const entry: FetchCacheEntry = {
			key: crypto.createHash("sha1").update(url).digest("hex").slice(0, 16),
			ts: Date.now(),
			url,
			title,
			body,
			source,
		};
		const next = kept.length > 0 ? kept + "\n" : "";
		await writeFileReal(file, next + JSON.stringify(entry) + "\n");
	} catch (e: any) {
		console.error(`[web-search] fetchCachePut failed: ${String(e?.message ?? e)}`);
	}
}

// ─────────────────────────────── web_fetch 安全校验 ───────────────────────────────

/** SSRF 防护：仅 https、拒绝环回/私有/保留地址段与本地主机名。 */
function isSafeFetchUrl(raw: string): boolean {
	const url = (raw ?? "").trim();
	if (!/^https:\/\//i.test(url)) return false;
	const m = /^https:\/\/\[?([^\]\/:?#]+)\]?(:[0-9]+)?([\/?#]|$)/i.exec(url);
	if (!m) return false;
	const host = m[1].toLowerCase();
	if (host === "localhost" || host.endsWith(".localhost") || host.endsWith(".local")) return false;
	// IPv6 字面量（含环回）
	if (host.includes(":")) {
		return !/^::1$/.test(host) && !/^[0:]+$/.test(host);
	}
	// IPv4 私有/保留段
	if (/^\d{1,3}(\.\d{1,3}){3}$/.test(host)) {
		const parts = host.split(".").map(Number);
		const [a, b] = parts;
		if (a === 0 || a === 10 || a === 127 || a >= 224) return false;
		if (a === 172 && b >= 16 && b <= 31) return false;
		if (a === 192 && b === 168) return false;
		if (a === 169 && b === 254) return false;
		if (a === 100 && b >= 64 && b <= 127) return false;
	}
	return true;
}

/** jina Reader（r.jina.ai/<url>）：URL → 干净 markdown。 */
async function jinaFetch(url: string): Promise<HttpOutcome> {
	return httpRequestRetry({
		url: `https://r.jina.ai/${encodeURIComponent(url)}`,
		headers: {
			Authorization: `Bearer ${settings.sources.jina.apiKey}`,
			Accept: "text/markdown",
		},
	});
}

/** Firecrawl（api.firecrawl.dev/v2/scrape）：URL → markdown（web_fetch 优先源）。 */
async function firecrawlFetch(url: string): Promise<HttpOutcome> {
	return httpRequestRetry({
		url: "https://api.firecrawl.dev/v2/scrape",
		method: "POST",
		headers: {
			Authorization: `Bearer ${settings.sources.firecrawl.apiKey}`,
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ url, formats: ["markdown"] }),
	});
}

function parseFirecrawl(body: string): { title: string; markdown: string } {
	try {
		const data = JSON.parse(body) as {
			success?: boolean;
			data?: { title?: string; markdown?: string };
		};
		if (data.success === false || !data.data) return { title: "", markdown: "" };
		return { title: String(data.data.title ?? ""), markdown: String(data.data.markdown ?? "") };
	} catch {
		return { title: "", markdown: "" };
	}
}

// ─────────────────────────────── 聚合主流程 ───────────────────────────────

interface SearchOutput {
	items: Array<{ title: string; url: string; description: string; position: number }>;
	meta: {
		mode: string;
		sourcesUsed: string[];
		sourcesOk: string[];
		sourcesFailed: string[];
		exhausted: string[];
		missingKeys: string[];
		unique: number;
		ms: number;
		cached: boolean;
	};
}

/** 整次搜索：缓存 → 选源 → 并行 → 合并 → 用量落盘。 */
async function runSearch(cwd: string, query: string, mode: "quick" | "deep", limit: number, fresh: boolean): Promise<SearchOutput> {
	const started = Date.now();

	// ① 缓存（fresh=true 跳过）
	if (!fresh) {
		const hit = cacheGet(cwd, query, mode, limit);
		if (hit) {
			return {
				items: hit.items,
				meta: {
					mode,
					sourcesUsed: [],
					sourcesOk: [],
					sourcesFailed: [],
					exhausted: [],
					missingKeys: [],
					unique: hit.items.length,
					ms: Date.now() - started,
					cached: true,
				},
			};
		}
	}

	// ② 选源 + 并行调用（allSettled 语义等价于 Hermes 的 as_completed，单源失败隔离）
	const sources = selectSources(mode);
	if (sources.length === 0) {
		const enabledKeys = Object.keys(SOURCE_META).filter((k) => settings.sources[k]?.enabled);
		const missingKeys = enabledKeys.filter((k) => !settings.sources[k]?.apiKey);
		const exhausted = enabledKeys.filter((k) => isExhausted(k));
		return {
			items: [],
			meta: {
				mode,
				sourcesUsed: [],
				sourcesOk: [],
				sourcesFailed: [],
				exhausted,
				missingKeys,
				unique: 0,
				ms: Date.now() - started,
				cached: false,
			},
		};
	}

	const perSourceLimit = Math.max(limit * PER_SOURCE_MULT, limit + 3);
	const tasks = sources.map((key) => {
		// 每源独立超时：Promise.race 兜底宿主超时未生效的情况
		return Promise.race([
			searchSource(key, query, perSourceLimit),
			sleep(SOURCE_TIMEOUT_MS + 1000).then(() => ({ source: key, items: [], ms: SOURCE_TIMEOUT_MS + 1000 } as SourceOutcome)),
		]);
	});
	const settled = await Promise.allSettled(tasks);
	const outcomes: SourceOutcome[] = [];
	for (const [idx, s] of settled.entries()) {
		if (s.status === "fulfilled" && s.value) outcomes.push(s.value);
		else outcomes.push({ source: sources[idx], items: [], ms: 0 });
	}

	// ③ 全局超时兜底（并行整体不应超过 GLOBAL_TIMEOUT_MS）
	if (Date.now() - started > GLOBAL_TIMEOUT_MS) {
		// 已返回的照常合并；未返回的已由 per-source race 收口
	}

	const ok = outcomes.filter((o) => o.items.length > 0);
	const failed = outcomes.filter((o) => o.items.length === 0).map((o) => o.source);
	const exhaustedNow = Object.keys(SOURCE_META).filter((k) => quota[k]?.exhaustedAt);

	// ④ 合并去重 + 排序标注
	const merged = mergeResults(ok);
	const items = finalize(merged, limit);

	// ⑤ 落盘用量 + 写缓存（无结果不缓存）
	await saveQuota(cwd);
	if (items.length > 0) await cachePut(cwd, query, mode, limit, items);

	return {
		items,
		meta: {
			mode,
			sourcesUsed: sources,
			sourcesOk: ok.map((o) => o.source),
			sourcesFailed: failed,
			exhausted: exhaustedNow,
			missingKeys: [],
			unique: merged.length,
			ms: Date.now() - started,
			cached: false,
		},
	};
}

/** Tavily 官方用量 API（GET /usage）：返回当前账单周期的真实消耗与额度。 */
async function tavilyUsage(): Promise<{ used: number; limit: number } | null> {
	const outcome = await httpRequestRetry({
		url: "https://api.tavily.com/usage",
		headers: { Authorization: `Bearer ${settings.sources.tavily.apiKey}` },
	});
	if (!outcome.ok) return null;
	try {
		const data = JSON.parse(outcome.body) as {
			key?: { usage?: number; limit?: number | null };
		};
		const u = data.key?.usage;
		if (typeof u === "number") {
			const l = data.key?.limit;
			return { used: u, limit: typeof l === "number" && l > 0 ? l : 1000 };
		}
	} catch {
		// 解析失败按无官方数据处理
	}
	return null;
}

/** 用量摘要文本：各源已用/剩余/耗尽状态（供 search_usage 工具返回）。 */
function formatUsageText(): string {
	const lines = ["搜索用量（各源免费额度）："];
	for (const [key, meta] of Object.entries(SOURCE_META)) {
		const s = settings.sources[key];
		const q = quota[key];
		if (!s?.enabled) {
			lines.push(`- ${meta.displayName}: 已禁用`);
			continue;
		}
		if (!s.apiKey) {
			lines.push(`- ${meta.displayName}: 未配置 Key`);
			continue;
		}
		const used = q?.used ?? 0;
		const total = q?.total ?? meta.quota.total;
		const unit = q?.unit ?? meta.quota.unit;
		const pct = total > 0 ? Math.round((used / total) * 100) : 0;
		const status = q?.exhaustedAt ? "（已耗尽）" : q?.lastErrorAt ? "（近期有失败，已自动让位）" : "";
		lines.push(`- ${meta.displayName}: ${used}/${total} ${unit}（${pct}%）${status}`);
	}
	for (const c of settings.custom) {
		if (!c.enabled || !c.name) continue;
		const q = quota[`custom:${c.name}`];
		lines.push(
			`- ${c.name}（自定义源）: 今日已调用 ${q?.callsToday ?? 0} 次${q?.exhaustedAt ? "（已耗尽）" : ""}`,
		);
	}
	lines.push("重置：Tavily / Exa / Firecrawl 每月重置；Jina 10M tokens 一次性；Serper 2500 次一次性。");
	lines.push("说明：用量按当前工作文件夹统计（跨项目不累计）；429 探测的额度耗尽会冷却 1 小时后自动重试；设置页「测试」按钮的探测也计入用量。");
	return lines.join("\n");
}

/** 把聚合结果序列化为模型可读的文本（比 JSON 省 token，模型直接可用）。 */
function formatSearchText(query: string, out: SearchOutput): string {	if (out.items.length === 0) {
		const parts = [`搜索 "${query}" 未返回结果。`];
		if (out.meta.missingKeys.length > 0) {
			parts.push(
				`未配置搜索源 API Key：${out.meta.missingKeys.join("、")}（在「设置 → 插件 → Web Search」中配置；Jina 提供免费额度）。` +
					"重试相同查询不会成功，请直接告知用户去配置。",
			);
		}
		if (out.meta.exhausted.length > 0) {
			parts.push(`以下源免费额度已耗尽（可到设置页查看/重置用量）：${out.meta.exhausted.join("、")}。`);
		}
		if (out.meta.sourcesFailed.length > 0) {
			parts.push(`失败源：${out.meta.sourcesFailed.join("、")}。`);
		}
		return parts.join("\n");
	}
	const lines = [`搜索 "${query}"（${out.meta.mode} 档，${out.meta.sourcesOk.length} 源，${out.meta.unique} 条去重后取 ${out.items.length} 条）：`];
	for (const item of out.items) {
		lines.push(
			`${item.position}. ${item.title}\n   ${item.url}\n   ${item.description}`,
		);
	}
	lines.push(
		`[源状态：${out.meta.sourcesOk.map((s) => `${s}✓`).join(" ")}${
			out.meta.sourcesFailed.length > 0 ? " " + out.meta.sourcesFailed.map((s) => `${s}✗`).join(" ") : ""
		}]${out.meta.cached ? "（缓存命中）" : ""} 耗时 ${out.meta.ms}ms`,
	);
	return lines.join("\n");
}

// ─────────────────────────────── 测试导出（仅供开发期单元验证；运行时不影响） ───────────────────────────────

export const __test = {
	normalizeUrl,
	titleSimilarity,
	parseJinaMarkdown,
	parseTavily,
	parseExa,
	parseSerper,
	parseFirecrawl,
	resolvePath,
	parseCustom,
	formatUsageText,
	isSafeFetchUrl,
	mergeResults,
	finalize,
	selectSources,
	remainingRatio,
	isExhausted,
	get settings() {
		return settings;
	},
	get quota() {
		return quota;
	},
	setState(nextSettings: Settings, nextQuota: Record<string, QuotaState>) {
		settings = nextSettings;
		quota = nextQuota;
	},
};

// ─────────────────────────────── 扩展入口 ───────────────────────────────

export default function (pi: ExtensionAPI) {
	piApi = pi;

	// ---- startup：预热配置与用量（信息性日志；失败不阻断） ----
	pi.on("startup", async (_event, ctx: any) => {
		try {
			settings = loadSettings();
			settingsLoaded = true;
			quota = loadQuota(typeof ctx?.cwd === "string" && ctx.cwd ? ctx.cwd : ".");
			console.log(
				`[web-search] startup ok, mode=${settings.mode}, sources=` +
					Object.entries(settings.sources)
						.filter(([, s]) => s.enabled)
						.map(([k, s]) => `${k}${s.apiKey ? "*" : "?"}`)
						.join(","),
			);
		} catch (e: any) {
			console.error(`[web-search] startup failed: ${String(e?.message ?? e)}`);
		}
	});

	// ---- ① web_search：多源聚合搜索 ----
	pi.registerTool({
		name: "web_search",
		label: "web_search",
		description:
			"联网搜索：并行调用多个搜索源（免费源优先），合并去重后按多源交叉验证强度排序，" +
			"description 带 [来源] 标注（如 [jina|tavily]）。免费额度耗尽会自动切换可用源；" +
			"相同查询在 TTL 内直接返回缓存。复杂问题建议拆分为多个具体查询分别调用。" +
			"搜索到链接后如需要正文细节，用 web_fetch 读取。mode=quick 仅免费源（Jina/Tavily/Exa/自定义源），mode=deep 含付费源（Serper）。",
		parameters: {
			type: "object",
			properties: {
				query: {
					type: "string",
					description: "搜索查询词（具体、单一主题效果好）",
				},
				mode: {
					type: "string",
					enum: ["quick", "deep"],
					description: '搜索档位：quick=仅免费源（默认）；deep=免费+付费源（覆盖更广，消耗付费配额）',
				},
				limit: {
					type: "number",
					description: "返回条数（默认 5，最大 20）",
				},
				fresh: {
					type: "boolean",
					description: "true 时跳过缓存强制重新搜索（默认 false）",
				},
			},
			required: ["query"],
		},

		async execute(_toolCallId, params: any, _signal, _onUpdate, ctx: any) {
			const query = String(params?.query ?? "").trim();
			if (!query) {
				return {
					content: [{ type: "text", text: "web_search: query 参数不能为空" }],
					details: { ok: false },
					isError: true,
				};
			}
			const cwd = typeof ctx?.cwd === "string" && ctx.cwd ? ctx.cwd : ".";
			// 工具参数显式指定时覆盖默认档位（quick/deep 均可）；未指定用设置默认
			const mode =
				params?.mode === "quick" || params?.mode === "deep" ? params.mode : settings.mode;
			const limit = Math.min(Math.max(Number(params?.limit) || 5, 1), 20);
			const fresh = Boolean(params?.fresh);

			// 每次执行前重读配置与用量（设置页改动即时生效）
			settings = loadSettings();
			quota = loadQuota(cwd);

			const out = await runSearch(cwd, query, mode, limit, fresh);
			const text = formatSearchText(query, out);
			return {
				content: [{ type: "text", text }],
				details: { ok: out.items.length > 0 || out.meta.sourcesOk.length > 0, ...out.meta },
			};
		},
	});

	// ---- ② web_fetch：读取单个网页正文（截断摘要） ----
	pi.registerTool({
		name: "web_fetch",
		label: "web_fetch",
		description:
			"读取单个网页的正文内容并返回摘要（Firecrawl 优先，Jina Reader 兜底；均按各自免费额度计费）。" +
			"当搜索结果 snippet 不足以回答细节问题时使用（如具体数据、文章全文）。" +
			"仅支持 https 地址；自动拒绝内网/本地地址。返回正文最多 8000 字符。",
		parameters: {
			type: "object",
			properties: {
				url: {
					type: "string",
					description: "要读取的网页 URL（必须 https:// 开头）",
				},
			},
			required: ["url"],
		},

		async execute(_toolCallId, params: any, _signal, _onUpdate, ctx: any) {
			const url = String(params?.url ?? "").trim();
			const cwd = typeof ctx?.cwd === "string" && ctx.cwd ? ctx.cwd : ".";
			if (!url) {
				return {
					content: [{ type: "text", text: "web_fetch: url 参数不能为空" }],
					details: { ok: false },
					isError: true,
				};
			}
			if (!isSafeFetchUrl(url)) {
				return {
					content: [
						{
							type: "text",
							text: `web_fetch: 拒绝访问该地址（仅支持公网 https://，不接受内网/本地地址）：${url.slice(0, 200)}`,
						},
					],
					details: { ok: false, reason: "unsafe_url" },
					isError: true,
				};
			}

			settings = loadSettings();
			quota = loadQuota(cwd);

			// 相同 URL 在 TTL 内直接返回缓存（避免重复抓取重复烧额度）
			const cached = fetchCacheGet(cwd, url);
			if (cached) {
				return {
					content: [
						{
							type: "text",
							text: `页面标题：${cached.title || "(无)"}\n正文（${cached.body.length} 字符，来源 ${cached.source}，缓存命中）：\n${cached.body}`,
						},
					],
					details: { ok: true, title: cached.title, chars: cached.body.length, source: cached.source, cached: true },
				};
			}

			const hasFirecrawl = Boolean(settings.sources.firecrawl?.apiKey) && !isExhausted("firecrawl");
			const hasJina = Boolean(settings.sources.jina?.apiKey) && !isExhausted("jina");
			if (!hasFirecrawl && !hasJina) {
				return {
					content: [
						{
							type: "text",
							text: "web_fetch: 未配置可用的提取源（Firecrawl / Jina 的 API Key 均未配置或额度已耗尽，请在设置页配置）",
						},
					],
					details: { ok: false, reason: "no_extract_source" },
					isError: true,
				};
			}

			// Firecrawl 优先（免费额度独立、抓取更全），失败兜底 jina Reader
			let outcome: HttpOutcome | null = null;
			let usedSource = "";
			if (hasFirecrawl) {
				usedSource = "firecrawl";
				outcome = await firecrawlFetch(url);
				if (!outcome.ok && hasJina) {
					usedSource = "jina";
					outcome = await jinaFetch(url);
				}
			} else {
				usedSource = "jina";
				outcome = await jinaFetch(url);
			}
			// 用量记录 + 额度探测
			const q = quota[usedSource];
			if (q) {
				q.callsToday = (q.callsToday ?? 0) + 1;
				if (q.unit === "calls" && q.used < q.total) q.used += 1;
			}
			if (!outcome.ok) {
				const exhausted = outcome.status === 429 || /quota|limit|exhaust/i.test(outcome.body.slice(0, 300));
				if (q && exhausted) {
					q.exhaustedAt = Date.now();
					q.used = q.total;
				}
				await saveQuota(cwd);
				return {
					content: [
						{
							type: "text",
							text: `web_fetch: 读取失败（${outcome.error ?? `HTTP ${outcome.status}`}${exhausted ? `，${usedSource} 免费额度可能已耗尽` : ""}）`,
						},
					],
					details: { ok: false, source: usedSource, status: outcome.status },
					isError: true,
				};
			}
			await saveQuota(cwd);

			// 提取标题与正文，截断输出
			let title = "";
			let md = outcome.body;
			if (usedSource === "firecrawl") {
				const parsed = parseFirecrawl(md);
				title = parsed.title;
				md = parsed.markdown;
			}
			if (!title) {
				const titleMatch = /^Title:\s*(.+)$/im.exec(md);
				if (titleMatch) title = titleMatch[1].trim();
				else {
					const h1 = /^#\s+(.+)$/m.exec(md);
					if (h1) title = h1[1].trim();
				}
			}
			const body = md
				.replace(/^Title:\s*.+$/im, "")
				.replace(/^URL:\s*.+$/im, "")
				.trim();
			const capped = body.length > FETCH_MAX_CHARS ? `${body.slice(0, FETCH_MAX_CHARS)}\n…（正文共 ${body.length} 字符，已截断）` : body;

			// 写入正文缓存（TTL 内相同 URL 直接命中，节省抓取额度）
			await fetchCachePut(cwd, url, title, capped, usedSource);

			return {
				content: [
					{
						type: "text",
						text: `页面标题：${title || "(无)"}\n正文（${body.length} 字符${body.length > FETCH_MAX_CHARS ? "，已截断" : ""}，来源 ${usedSource}）：\n${capped}`,
					},
				],
				details: { ok: true, title, chars: body.length, source: usedSource },
			};
		},
	});

	// ---- ③ search_usage：用量与免费额度查询 ----
	pi.registerTool({
		name: "search_usage",
		label: "search_usage",
		description:
			"查看搜索插件的用量与免费额度状态（各源已用/剩余/是否耗尽/今日调用）。" +
			"当用户询问搜索额度、免费次数、用量消耗，或 web_search 因额度失败时使用。",
		parameters: {
			type: "object",
			properties: {},
			required: [],
		},

		async execute(_toolCallId, _params: any, _signal, _onUpdate, ctx: any) {
			const cwd = typeof ctx?.cwd === "string" && ctx.cwd ? ctx.cwd : ".";
			settings = loadSettings();
			quota = loadQuota(cwd);
			// Tavily 官方用量校准（真实账单数字，覆盖自我统计）
			if (settings.sources.tavily?.apiKey) {
				const tv = await tavilyUsage();
				if (tv && quota.tavily) {
					quota.tavily.used = tv.used;
					quota.tavily.total = tv.limit;
					if (tv.used >= tv.limit) quota.tavily.exhaustedAt = Date.now();
					await saveQuota(cwd);
				}
			}
			return {
				content: [{ type: "text", text: formatUsageText() }],
				details: { ok: true, sources: Object.keys(SOURCE_META).length + settings.custom.filter((c) => c.enabled && c.name).length },
			};
		},
	});
}
