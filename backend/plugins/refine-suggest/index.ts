/**
 * Refine Suggest —— BoenMind 自我改进建议采集插件（借鉴 Prime Agent /refine 的心智）。
 *
 * 机制（宿主审批模式，安全边界在宿主侧）：
 * - 代理完成任务后，若发现某 skill 的 description 或系统提示词存在误导/低效，
 *   可调用 submit_refinement_suggestions 提交结构化建议；
 * - 本插件只是一个"记录桩"：工具调用参数会在 bm-server 的 toolCallStart
 *   事件流中被截获并写入 refinement_suggestions 表（status=pending），
 *   工具本身不落任何状态、不直接生效；
 * - 用户在设置页"改进建议"中审批：批准后由 bm-server 修改 SKILL.md 描述
 *   （或追加系统提示词段，均带备份可回滚），拒绝则丢弃。
 *
 * 与 Prime Agent /refine 的差异（有意为之）：
 * - 代理只"提建议"不"改手册"——审批权始终在用户/宿主，避免把坏经验
 *   refine 进知识库（上游 Factorio 演示即因此翻车）。
 */
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "submit_refinement_suggestions",
		label: "submit_refinement_suggestions",
		description:
			"提交一条改进建议：针对某个 skill 的描述（description）或系统提示词的改进意见。" +
			"仅在任务完成、且你确实发现 skill 描述/提示词有误导、不准确或明显可改进之处时调用；" +
			"没有可提的建议就绝不调用。建议只被记录，由用户审批后才生效——不要声称已生效。" +
			"target 取值：\"skill:<skill-id>\"（如 skill:web-scraping，指启用中的 skill）或 \"system_prompt\"。" +
			"quote 必须是目标描述中存在的原文片段（不要改写），suggested 给出替换/追加后的完整文本，reason 说明为什么。" +
			"同一问题不要重复提交；若发现多个独立问题可分多次调用。",
		parameters: {
			type: "object",
			properties: {
				target: {
					type: "string",
					description: '"skill:<id>" 或 "system_prompt"',
				},
				quote: {
					type: "string",
					description: "目标描述中需修改的原文片段（须与原文一致）",
				},
				suggested: {
					type: "string",
					description: "建议的替换/追加文本（完整描述）",
				},
				reason: {
					type: "string",
					description: "提出该建议的原因（基于本次任务的实际观察）",
				},
			},
			required: ["target", "quote", "suggested", "reason"],
		},

		async execute(_toolCallId, params: any, _signal, _onUpdate, _ctx) {
			const target = String(params?.target ?? "").trim();
			const quote = String(params?.quote ?? "").trim();
			const suggested = String(params?.suggested ?? "").trim();
			const reason = String(params?.reason ?? "").trim();
			if (!target || !quote || !suggested || !reason) {
				return {
					content: [
						{
							type: "text",
							text: "submit_refinement_suggestions: 四个参数 target/quote/suggested/reason 都必填，请补齐后重试。",
						},
					],
					details: { ok: false, reason: "missing_arguments" },
					isError: true,
				};
			}
			if (!target.startsWith("skill:") && target !== "system_prompt") {
				return {
					content: [
						{
							type: "text",
							text: 'submit_refinement_suggestions: target 必须是 "skill:<id>" 或 "system_prompt"。',
						},
					],
					details: { ok: false, reason: "invalid_target" },
					isError: true,
				};
			}
			// 本插件是记录桩：宿主（bm-server）在 toolCallStart 事件流中截获参数入库。
			return {
				content: [
					{
						type: "text",
						text: "建议已记录，等待用户审批。注意：在用户批准之前建议不会生效，请不要在后续回复中把它当作已生效的事实。",
					},
				],
				details: { ok: true, recorded: true, pendingApproval: true },
				isError: false,
			};
		},
	});
}
