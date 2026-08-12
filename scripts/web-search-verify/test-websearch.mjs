/**
 * web-search 插件纯函数单元验证（开发期脚本，不进入产物）。
 * 利用 Node 24 原生 TS type-stripping 直接加载 index.ts（__test 导出）：
 * URL 规范化 / 标题相似去重 / markdown 解析 / JSON 解析 / SSRF 防护 / 选源 / 合并排序。
 */
const SRC = "/Users/boen/.zcode/workspace/BoenMind/backend/plugins/web-search/index.ts";
const { __test } = await import(SRC);

let passed = 0;
let failed = 0;
function check(name, cond, extra = "") {
	if (cond) {
		passed++;
		console.log(`  ✓ ${name}`);
	} else {
		failed++;
		console.log(`  ✗ ${name} ${extra}`);
	}
}

// ── normalizeUrl ──
console.log("normalizeUrl");
check("去 www + 小写 host", __test.normalizeUrl("https://WWW.Example.com/Path") === "example.com/Path");
check("去尾部斜杠", __test.normalizeUrl("https://example.com/a/b/") === "example.com/a/b");
check("丢跟踪参数 utm/fbclid", __test.normalizeUrl("https://example.com/p?utm_source=x&a=1&fbclid=abc") === "example.com/p?a=1");
check("query 排序（同页不同顺序归一）", __test.normalizeUrl("https://example.com/p?b=2&a=1") === __test.normalizeUrl("https://example.com/p?a=1&b=2"));
check("保留非跟踪 query", __test.normalizeUrl("https://example.com/s?q=rust&lang=zh") === "example.com/s?lang=zh&q=rust");
check("空输入", __test.normalizeUrl("  ") === "");
check("无协议原样返回", __test.normalizeUrl("example.com") === "example.com");

// ── titleSimilarity ──
console.log("titleSimilarity");
check("相同标题 = 1", __test.titleSimilarity("Rust 语言入门指南", "Rust 语言入门指南") > 0.99);
check("转载变体 > 0.7", __test.titleSimilarity("巴黎备忘录2025白名单发布", "巴黎备忘录 2025 白名单发布！") > 0.7);
check("无关标题 < 0.5", __test.titleSimilarity("巴黎备忘录白名单", "苹果发布新款手机") < 0.5);

// ── parseJinaMarkdown ──
console.log("parseJinaMarkdown");
const jinaMd = `一些前言文字
[结果一标题](https://a.example.com/x)
这是结果一的描述文字，跨行继续。

### 结果二标题
https://b.example.com/y
结果二的描述。

[结果三标题](https://c.example.com/z)
描述三。`;
const parsed = __test.parseJinaMarkdown(jinaMd, 10);
check("解析 3 条", parsed.length === 3, `got ${parsed.length}`);
check("链接格式标题/URL", parsed[0].title === "结果一标题" && parsed[0].url === "https://a.example.com/x");
check("### 格式解析 URL", parsed[1].title === "结果二标题" && parsed[1].url === "https://b.example.com/y");
check("position 递增", parsed[2].position === 3);
check("limit 生效", __test.parseJinaMarkdown(jinaMd, 2).length === 2);

// ── parseTavily / parseSerper ──
console.log("parseTavily / parseSerper");
check("tavily 解析", __test.parseTavily(JSON.stringify({ results: [{ title: "T1", url: "https://t.example.com", content: "内容" }] })).length === 1);
check("tavily 非法 JSON 空数组", __test.parseTavily("not json").length === 0);
check("serper 解析", __test.parseSerper(JSON.stringify({ organic: [{ title: "S1", link: "https://s.example.com", snippet: "片段" }] })).length === 1);
check("serper 非法 JSON 空数组", __test.parseSerper("").length === 0);

// ── parseExa / parseFirecrawl ──
console.log("parseExa / parseFirecrawl");
check("exa 解析", __test.parseExa(JSON.stringify({ results: [{ title: "E1", url: "https://e.example.com", text: "内容" }] })).length === 1);
check("exa 标题带日期", __test.parseExa(JSON.stringify({ results: [{ title: "E2", url: "https://e.example.com/2", text: "x", publishedDate: "2026-08-01" }] }))[0].title.includes("2026-08-01"));
check("exa 非法 JSON 空数组", __test.parseExa("").length === 0);
check("firecrawl 解析", __test.parseFirecrawl(JSON.stringify({ success: true, data: { title: "F1", markdown: "# 标题\n正文" } })).markdown.length > 0);
check("firecrawl success=false 空", __test.parseFirecrawl(JSON.stringify({ success: false })).markdown === "");
check("firecrawl 非法 JSON 空", __test.parseFirecrawl("not json").markdown === "");

// ── resolvePath / parseCustom（自定义源） ──
console.log("resolvePath / parseCustom");
check("点分路径取值", __test.resolvePath({ a: { b: [1, 2] } }, "a.b")[1] === 2);
check("点分路径缺失返回 undefined", __test.resolvePath({ a: 1 }, "a.b.c") === undefined);
const customSrc = { enabled: true, name: "mysearx", url: "https://searx.example.com/search?q={query}&format=json", apiKeyHeader: "", apiKey: "", resultsPath: "data.list", titlePath: "title", urlPath: "link", descPath: "content" };
const customBody = JSON.stringify({ data: { list: [{ title: "C1", link: "https://c.example.com/1", content: "内容1" }, { title: "C2", link: "https://c.example.com/2" }] } });
const customParsed = __test.parseCustom(customBody, customSrc);
check("自定义源解析 2 条", customParsed.length === 2, `got ${customParsed.length}`);
check("自定义源字段映射", customParsed[0].title === "C1" && customParsed[0].url === "https://c.example.com/1" && customParsed[0].description === "内容1");
check("自定义源缺 desc 为空串", customParsed[1].description === "");
check("自定义源无 url 元素跳过", __test.parseCustom(JSON.stringify({ results: [{ title: "X" }] }), { ...customSrc, resultsPath: "results" }).length === 0);
check("自定义源非法 JSON 空", __test.parseCustom("bad", customSrc).length === 0);


// ── isSafeFetchUrl（SSRF 防护） ──
console.log("isSafeFetchUrl");
check("公网 https 放行", __test.isSafeFetchUrl("https://example.com/page?q=1"));
check("http 拒绝", !__test.isSafeFetchUrl("http://example.com"));
check("localhost 拒绝", !__test.isSafeFetchUrl("https://localhost:8080/x"));
check("127.0.0.1 拒绝", !__test.isSafeFetchUrl("https://127.0.0.1/x"));
check("10.x 拒绝", !__test.isSafeFetchUrl("https://10.0.0.5/x"));
check("172.16-31 拒绝", !__test.isSafeFetchUrl("https://172.16.3.4/x"));
check("192.168 拒绝", !__test.isSafeFetchUrl("https://192.168.1.1/x"));
check("169.254 拒绝", !__test.isSafeFetchUrl("https://169.254.169.254/latest/meta-data"));
check("100.64 CGNAT 拒绝", !__test.isSafeFetchUrl("https://100.64.1.1/x"));
check("IPv6 环回拒绝", !__test.isSafeFetchUrl("https://[::1]/x"));
check(".local 拒绝", !__test.isSafeFetchUrl("https://printer.local/x"));
check("公网 IP 放行", __test.isSafeFetchUrl("https://8.8.8.8/x"));
check("空输入拒绝", !__test.isSafeFetchUrl(""));
check("无协议拒绝", !__test.isSafeFetchUrl("example.com/x"));

// ── selectSources / 用量 ──
console.log("selectSources（选源与自动切换）");
const mkSettings = (over = {}) => ({
	mode: "quick",
	cacheTtlSeconds: 600,
	sources: {
		jina: { enabled: true, apiKey: "k1" },
		tavily: { enabled: true, apiKey: "k2" },
		exa: { enabled: true, apiKey: "k3" },
		firecrawl: { enabled: true, apiKey: "k4" },
		serper: { enabled: false, apiKey: "" },
	},
	custom: [
		{ enabled: false, name: "", url: "", apiKeyHeader: "", apiKey: "", resultsPath: "results", titlePath: "title", urlPath: "url", descPath: "description" },
		{ enabled: false, name: "", url: "", apiKeyHeader: "", apiKey: "", resultsPath: "results", titlePath: "title", urlPath: "url", descPath: "description" },
	],
	...over,
});
const mkQuota = (over = {}) => {
	const base = {
		jina: { used: 0, total: 10_000_000, unit: "tokens", callsToday: 0, today: "2026-08-12" },
		tavily: { used: 0, total: 1000, unit: "calls", reset: "monthly", callsToday: 0, today: "2026-08-12" },
		exa: { used: 0, total: 1000, unit: "calls", reset: "monthly", callsToday: 0, today: "2026-08-12" },
		firecrawl: { used: 0, total: 500, unit: "calls", reset: "monthly", callsToday: 0, today: "2026-08-12" },
		serper: { used: 0, total: 2500, unit: "calls", callsToday: 0, today: "2026-08-12" },
	};
	for (const [k, v] of Object.entries(over)) base[k] = { ...base[k], ...v };
	return base;
};

// quick 档只选免费源
__test.setState(mkSettings(), mkQuota());
const quickSel = __test.selectSources("quick");
check("quick 档选 2 个免费源", quickSel.length === 2 && quickSel.every((k) => ["jina", "tavily"].includes(k)), `got ${quickSel}`);

// deep 档含付费源（serper 需启用且有 key）
__test.setState(mkSettings({ sources: { jina: { enabled: true, apiKey: "k1" }, tavily: { enabled: true, apiKey: "k2" }, exa: { enabled: true, apiKey: "k3" }, firecrawl: { enabled: true, apiKey: "k4" }, serper: { enabled: true, apiKey: "k5" } } }), mkQuota());
const deepSel = __test.selectSources("deep");
check("deep 档选 4 源（3 免费 + serper）", deepSel.length === 4 && deepSel.includes("serper"), `got ${deepSel}`);

// 未配置 key 的源不选
__test.setState(mkSettings({ sources: { jina: { enabled: true, apiKey: "" }, tavily: { enabled: true, apiKey: "k2" }, exa: { enabled: true, apiKey: "" }, firecrawl: { enabled: true, apiKey: "" }, serper: { enabled: false, apiKey: "" } } }), mkQuota());
const noKeySel = __test.selectSources("quick");
check("无 key 源被跳过", noKeySel.length === 1 && noKeySel[0] === "tavily", `got ${noKeySel}`);

// 额度耗尽自动切换：tavily 用光 → 自动换到其他免费源
__test.setState(mkSettings(), mkQuota({ tavily: { used: 1000, exhaustedAt: Date.now() } }));
const exhaustedSel = __test.selectSources("quick");
check("耗尽源自动跳过", exhaustedSel.length === 2 && !exhaustedSel.includes("tavily"), `got ${exhaustedSel}`);

// 全部免费源耗尽 → 空
__test.setState(mkSettings(), mkQuota({ jina: { exhaustedAt: Date.now() }, tavily: { exhaustedAt: Date.now() }, exa: { exhaustedAt: Date.now() } }));
check("全部耗尽返回空", __test.selectSources("quick").length === 0);

// 平均使用：tavily/exa 今日调用多 → jina 优先
__test.setState(mkSettings(), mkQuota({ tavily: { callsToday: 50 }, exa: { callsToday: 40 } }));
const order = __test.selectSources("quick");
check("调用次数少的优先", order[0] === "jina", `got ${order}`);

// 月度重置：上月耗尽本月复活
__test.setState(mkSettings(), mkQuota({ tavily: { used: 1000, exhaustedAt: Date.parse("2026-07-20") } }));
check("月度额度跨月复活", __test.isExhausted("tavily") === false);

// 失败惩罚：jina 最近失败 → 选源跳过它（有其他可用源时）
__test.setState(mkSettings(), mkQuota({ jina: { lastError: "timeout", lastErrorAt: Date.now() - 1000 } }));
const penalized = __test.selectSources("quick");
check("失败源惩罚让位", !penalized.includes("jina"), `got ${penalized}`);

// 惩罚窗口过期后恢复
__test.setState(mkSettings(), mkQuota({ jina: { lastError: "timeout", lastErrorAt: Date.now() - 10 * 60_000 } }));
const recovered = __test.selectSources("quick");
check("惩罚窗口过期恢复", recovered.includes("jina"), `got ${recovered}`);

// 全部在惩罚窗口内 → 仍尽力选（不空手）
__test.setState(mkSettings(), mkQuota({
  jina: { lastErrorAt: Date.now() - 1000 },
  tavily: { lastErrorAt: Date.now() - 1000 },
  exa: { lastErrorAt: Date.now() - 1000 },
}));
check("全部惩罚中仍尽力选", __test.selectSources("quick").length === 2);

// 自定义源参与选源（禁用两个内置源后 custom 应入选 quick 档）
__test.setState(mkSettings({
  sources: { jina: { enabled: false, apiKey: "" }, tavily: { enabled: true, apiKey: "k2" }, exa: { enabled: false, apiKey: "" }, firecrawl: { enabled: false, apiKey: "" }, serper: { enabled: false, apiKey: "" } },
  custom: [{ ...customSrc, enabled: true }, { enabled: false, name: "", url: "", apiKeyHeader: "", apiKey: "", resultsPath: "results", titlePath: "title", urlPath: "url", descPath: "description" }],
}), mkQuota());
const withCustom = __test.selectSources("quick");
check("自定义源进入 quick 选源", withCustom.includes("custom:mysearx"), `got ${withCustom}`);

// 用量摘要格式（构造 quota 状态）
__test.setState(mkSettings(), mkQuota({ tavily: { used: 50, total: 1000, unit: "calls", reset: "monthly", callsToday: 3, today: "2026-08-12" } }));
const usageText = __test.formatUsageText();
check("用量摘要含源与百分比", usageText.includes("Tavily: 50/1000") && usageText.includes("5%"), usageText);
check("用量摘要含重置说明", usageText.includes("每月重置"));

// ── mergeResults + finalize ──
console.log("mergeResults + finalize");
const srcA = [
	{ title: "Rust 异步编程", url: "https://a.example.com/rust-async", description: "全面教程", position: 1 },
	{ title: "Tavily 介绍", url: "https://tavily.com", description: "搜索 API", position: 2 },
];
const srcB = [
	{ title: "Rust 异步编程", url: "https://www.A.example.com/rust-async?utm_source=news", description: "全面教程（转载）", position: 1 },
	{ title: "Rust 异步编程", url: "https://b.example.com/rust-async-copy", description: "全面教程全文转载", position: 1 },
];
// srcB 的两条都是转载/同页变体：www+utm 同页按 URL 合并，b.example.com 同标题按相似度合并
// → 最终 2 条：Rust 异步编程（jina+tavily 双源）、Tavily 介绍（仅 jina）
const merged = __test.mergeResults([
	{ source: "jina", items: srcA, ms: 100 },
	{ source: "tavily", items: srcB, ms: 100 },
]);
check("URL 去重 + 转载合并后剩 2 条", merged.length === 2, `got ${merged.length}: ${merged.map((m) => m.title).join(" | ")}`);
check("跨源同页来源集 = 2", merged.some((m) => m.sources.size === 2 && m.sources.has("jina") && m.sources.has("tavily")));
check("转载条目并入同标题条目", merged.filter((m) => m.title === "Rust 异步编程").length === 1 && merged.find((m) => m.title === "Rust 异步编程")?.sources.size === 2);

const final = __test.finalize(merged, 10);
check("finalize 标注来源前缀", final[0].description.startsWith("[") && final[0].description.includes("|"));
check("finalize 排序：多源条目在最前", final[0].title === "Rust 异步编程");
check("finalize 截断", final[0].description.length <= 350);

console.log(`\n结果：${passed} 通过，${failed} 失败`);
process.exit(failed > 0 ? 1 : 0);
