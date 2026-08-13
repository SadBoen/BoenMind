/**
 * pdf-omni —— PDF 智能解析（TS 薄壳插件）。
 *
 * 职责：注册 parse_pdf 工具 + 参数透传。全部重活（流式上传、轮询、PDF 操作、
 * 级联分桶、交叉验证、后处理、多 key 预算）由 bm-server 的 Rust 核心完成——
 * 插件经 loopback 调 `POST /api/plugins/pdf-omni/parse`（宿主端点）。
 *
 * 架构归属（2026-08 决策）：
 * - TS 壳：工具 schema、参数校验、设置页（API keys 走 extension.json settings）
 * - Rust 核心（bm-server/src/pdf_omni/）：引擎客户端 + 编排 + lopdf/zip/image
 * - API keys 由端点从插件设置文件读取（单源），不在 loopback 上传
 *
 * 能力边界（QuickJS 沙箱）：npm 包不可导入、pi.http 仅 GET/POST——本插件只
 * 用 pi.http 调本地端点，零依赖。
 */
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

let piApi: any = null;

/** bm-server 端口：与宿主同进程环境变量（默认 17321） */
function endpoint(): string {
	const port = (typeof process?.env?.BOENMIND_PORT === "string" && process.env.BOENMIND_PORT) || "17321";
	return `http://127.0.0.1:${port}/api/plugins/pdf-omni/parse`;
}

/** 工具执行超时：轮询上限 600s × 2（上传+解析+级联可能多轮），宿主另有 15min prompt 兜底 */
const EXEC_TIMEOUT_MS = 15 * 60 * 1000;

const TOOL_DESCRIPTION =
	"解析 PDF 为 Markdown（版面/表格/公式保真）。引擎: MinerU(默认, 1000页/天) / " +
	"LlamaParse(1万 credits/月, 默认 agentic 档)。cascade=True 时 MinerU 先解析, 表格/低置信度页 " +
	"自动交给 LlamaParse 增强; verify=True 时双引擎交叉验证(Jaccard 报告); refine 默认开启 " +
	"(后处理修伪标题/页眉页脚/残留标记)。适合论文、财报、合同、扫描件等复杂文档。用户要求解析/读取/转换 PDF 时使用。";

export default function (pi: ExtensionAPI) {
	piApi = pi;

	pi.registerTool({
		name: "parse_pdf",
		label: "parse_pdf",
		description: TOOL_DESCRIPTION,
		parameters: {
			type: "object",
			properties: {
				file: {
					type: "string",
					description: "本地 PDF 路径（工作区相对/绝对），或 http(s) 公网 URL（仅 MinerU 支持 URL）",
				},
				engine: {
					type: "string",
					enum: ["mineru", "llamaparse", "auto"],
					description: "解析引擎。auto=MinerU 优先（失败不再自动降级）",
				},
				tier: {
					type: "string",
					enum: ["fast", "cost_effective", "agentic", "agentic_plus"],
					description: "LlamaParse 档位, 默认 agentic(10 credits/页, 质量最优)。fast 不输出 Markdown 勿用",
				},
				cascade: {
					type: "boolean",
					description: "级联增强(engine=mineru 时): MinerU 先解析, 表格/公式/图表页自动切出交给 LlamaParse 重解析（三级分桶，省 credits）",
				},
				verify: {
					type: "boolean",
					description: "是否用另一引擎跑第二遍做交叉验证（消耗双倍额度, 建议仅重要文档）",
				},
				model_version: {
					type: "string",
					enum: ["pipeline", "vlm"],
					description: "MinerU 模型版本, 默认 vlm(推荐, 精度最高)",
				},
				is_ocr: {
					type: "boolean",
					description: "强制 OCR（扫描件/图片型 PDF 建议开启）",
				},
				language: {
					type: "string",
					description: "文档语言, 默认 ch(中英)。可选 en/japan/korean/chinese_cht 等",
				},
				out_dir: {
					type: "string",
					description: "Markdown 输出目录（工作区相对路径, 默认工作区根）",
				},
				refine: {
					type: "boolean",
					description: "是否应用后处理（修伪标题/页眉页脚/空表/残留标记）, 默认 true",
				},
			},
			required: ["file"],
		},

		async execute(_toolCallId, params: any, _signal, _onUpdate, _ctx) {
			const file = String(params?.file ?? "").trim();
			if (!file) {
				return {
					content: [{ type: "text", text: "parse_pdf: file 参数不能为空" }],
					details: { ok: false },
					isError: true,
				};
			}
			// 仅透传显式提供的参数（Rust 侧对缺省参数用其默认值）
			const body: Record<string, unknown> = { file };
			for (const key of [
				"engine", "tier", "cascade", "verify", "model_version",
				"is_ocr", "language", "out_dir", "refine",
			]) {
				if (params?.[key] !== undefined) {
					body[key] = params[key];
				}
			}
			try {
				const res = await piApi.http({
					url: endpoint(),
					method: "POST",
					headers: { "Content-Type": "application/json" },
					body: JSON.stringify(body),
					timeout: EXEC_TIMEOUT_MS,
				});
				const status = Number(res?.status) || 0;
				const text = typeof res?.body === "string" ? res.body : "";
				if (status < 200 || status >= 300) {
					return {
						content: [{ type: "text", text: `parse_pdf: 宿主端点 HTTP ${status}: ${text.slice(0, 300)}` }],
						details: { ok: false, status },
						isError: true,
					};
				}
				// 宿主返回工具约定的 JSON（success/error 已结构化）
				return {
					content: [{ type: "text", text }],
					details: { ok: true, status },
				};
			} catch (e: any) {
				return {
					content: [{
						type: "text",
						text: `parse_pdf: 宿主调用失败: ${String(e?.message ?? e ?? "unknown error")}`,
					}],
					details: { ok: false },
					isError: true,
				};
			}
		},
	});
}
