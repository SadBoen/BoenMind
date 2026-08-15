/**
 * Role Definition —— BoenMind 角色定义插件。
 *
 * 角色 = 助手的人格/职责设定（一段系统提示风格的指令文本）。用户或模型
 * 通过 `role` 工具创建/切换角色；宿主的角色注入挂点（bm-server/src/roles.rs）
 * 在每次模型请求前把**当前激活角色**追加进系统消息——角色切换即时生效，
 * 无需重建会话。
 *
 * 数据文件：`~/.boenmind/roles.json`（插件独占写入；宿主只读注入）。
 * os.homedir() 由宿主对齐 $BOENMIND_HOME（见 bm-compat build_node_os_module），
 * 与宿主 app_dir() 指向同一位置。QuickJS 的 node:fs 写入是 VFS 内存层
 * （不落盘），真实持久化必须走宿主工具 pi.tool("write")——与 ctx-compactor
 * 同款约定。
 *
 * 文件结构：{ "active": "<角色id>|null", "roles": [{ "id", "name", "prompt" }] }
 *
 * 边界（注入是有界字符的原则，对齐记忆注入）：
 * - id：1~48 字符，仅小写字母/数字/中划线/下划线；
 * - name：1~64 字符；
 * - prompt：1~2000 字符（宿主注入上限同此值）。
 */

// 宿主 API 引用（default export 里赋值；pi.tool 持久化调用需要）
let piApi: any;

import * as os from "node:os";

// ─────────────────────────────── 常量与校验 ───────────────────────────────
const ROLES_FILE = () => `${os.homedir()}/.boenmind/roles.json`;
const MAX_ID_CHARS = 48;
const MAX_NAME_CHARS = 64;
const MAX_PROMPT_CHARS = 2000;
const ID_RE = /^[a-z0-9][a-z0-9_-]*$/;

/** 角色列表 + 当前激活 id。 */
interface RoleStore {
	active: string | null;
	roles: { id: string; name: string; prompt: string }[];
}

function emptyStore(): RoleStore {
	return { active: null, roles: [] };
}

/** 读取角色库；文件缺失/损坏 → 空库（损坏文件不覆盖写，见 saveStore）。 */
async function loadStore(): Promise<RoleStore> {
	try {
		const read = (await piApi.tool("read", { path: ROLES_FILE() })) as any;
		const text = blockText(read);
		if (!text) return emptyStore();
		const parsed = JSON.parse(text) as RoleStore;
		if (!parsed || typeof parsed !== "object" || !Array.isArray(parsed.roles)) {
			return emptyStore();
		}
		const roles = parsed.roles.filter(
			(r: any) => r && typeof r.id === "string" && typeof r.name === "string" && typeof r.prompt === "string"
		);
		const active = roles.some((r) => r.id === parsed.active) ? parsed.active : null;
		return { active, roles };
	} catch {
		return emptyStore();
	}
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

/** 全量保存（读-改-写；宿主 write 工具真实落盘）。 */
async function saveStore(store: RoleStore): Promise<void> {
	const next = JSON.stringify(store, null, 2);
	await piApi.tool("write", { path: ROLES_FILE(), content: next + "\n" });
}

/** 校验角色字段，返回错误消息或 null。 */
function validateRole(id: string, name: string, prompt: string): string | null {
	if (!ID_RE.test(id) || id.length > MAX_ID_CHARS) {
		return `角色 id 必须为 1~${MAX_ID_CHARS} 字符的小写字母/数字/中划线/下划线（当前: ${id}）`;
	}
	if (name.length === 0 || name.length > MAX_NAME_CHARS) {
		return `角色名称须为 1~${MAX_NAME_CHARS} 字符`;
	}
	if (prompt.length === 0 || prompt.length > MAX_PROMPT_CHARS) {
		return `角色提示词须为 1~${MAX_PROMPT_CHARS} 字符`;
	}
	return null;
}

/** 角色库人类可读摘要（list 输出用）。 */
function describeStore(store: RoleStore): string {
	if (store.roles.length === 0) {
		return "尚未定义任何角色。用 role(op:create) 创建第一个角色。";
	}
	const lines = store.roles.map((r) => {
		const marker = r.id === store.active ? "●" : "○";
		return `${marker} ${r.id}（${r.name}）`;
	});
	const active = store.active
		? store.roles.find((r) => r.id === store.active)?.name ?? store.active
		: "未设置";
	return `当前角色：${active}\n角色列表：\n${lines.join("\n")}`;
}

// ─────────────────────────────── 工具执行 ───────────────────────────────
/**
 * 分发 role 工具操作。参数已按 schema 约束（op 必填，其余按 op 取）。
 * 持久化失败时返回错误（角色未生效必须让模型知道，不能静默丢）。
 */
async function runRole(op: string, id: string | undefined, name: string | undefined, prompt: string | undefined) {
	const store = await loadStore();

	switch (op) {
		case "list": {
			return { text: describeStore(store), details: store };
		}
		case "show": {
			if (!store.active) {
				return { text: "当前未设置角色。可用 role(op:set) 激活一个角色，或用 role(op:create) 创建。", details: { active: null } };
			}
			const role = store.roles.find((r) => r.id === store.active);
			return {
				text: `当前角色：${role?.name ?? store.active}\n${role?.prompt ?? ""}`,
				details: role ?? null,
			};
		}
		case "set": {
			if (!id) return { error: "set 需要 id 参数" };
			if (!store.roles.some((r) => r.id === id)) {
				return { error: `角色不存在: ${id}（可先 role(op:list) 查看，或 role(op:create) 创建）` };
			}
			store.active = id;
			await saveStore(store);
			return { text: `已切换到角色：${id}（立即生效）`, details: { active: id } };
		}
		case "clear": {
			if (!store.active) {
				return { text: "当前本就未设置角色。", details: { active: null } };
			}
			store.active = null;
			await saveStore(store);
			return { text: "已清除角色（回到默认助手行为）", details: { active: null } };
		}
		case "create": {
			if (!id || !name || !prompt) return { error: "create 需要 id/name/prompt 参数" };
			const invalid = validateRole(id, name, prompt);
			if (invalid) return { error: invalid };
			if (store.roles.some((r) => r.id === id)) {
				return { error: `角色已存在: ${id}（可用 update 修改，或 delete 后重建）` };
			}
			store.roles.push({ id, name, prompt });
			await saveStore(store);
			return { text: `角色已创建：${id}（${name}）。要立即生效请 role(op:set, id:${id})`, details: { created: id } };
		}
		case "update": {
			if (!id) return { error: "update 需要 id 参数" };
			const role = store.roles.find((r) => r.id === id);
			if (!role) return { error: `角色不存在: ${id}` };
			if (name !== undefined) {
				if (name.length === 0 || name.length > MAX_NAME_CHARS) return { error: `角色名称须为 1~${MAX_NAME_CHARS} 字符` };
				role.name = name;
			}
			if (prompt !== undefined) {
				if (prompt.length === 0 || prompt.length > MAX_PROMPT_CHARS) return { error: `角色提示词须为 1~${MAX_PROMPT_CHARS} 字符` };
				role.prompt = prompt;
			}
			await saveStore(store);
			return { text: `角色已更新：${id}`, details: { updated: id } };
		}
		case "delete": {
			if (!id) return { error: "delete 需要 id 参数" };
			const idx = store.roles.findIndex((r) => r.id === id);
			if (idx === -1) return { error: `角色不存在: ${id}` };
			const wasActive = store.active === id;
			store.roles.splice(idx, 1);
			if (wasActive) store.active = null;
			await saveStore(store);
			return {
				text: `角色已删除：${id}${wasActive ? "（原激活角色已随之清除）" : ""}`,
				details: { deleted: id },
			};
		}
		default:
			return { error: `未知操作: ${op}` };
	}
}

// ─────────────────────────────── 扩展入口 ───────────────────────────────
export default function (pi: any) {
	piApi = pi;

	pi.registerTool({
		name: "role",
		label: "role",
		description:
			"角色定义与管理：创建/切换/查看/删除助手的角色（人格与职责设定）。" +
			"op=list 查看全部角色与当前激活项；op=set 激活某个角色（立即生效，宿主注入系统提示）；" +
			"op=clear 清除角色；op=create 创建（id 为小写字母数字连字符，prompt 为角色指令文本，上限 2000 字符）；" +
			"op=update 修改名称或提示词；op=delete 删除；op=show 查看当前角色全文。",
		parameters: {
			type: "object",
			properties: {
				op: {
					type: "string",
					enum: ["list", "show", "set", "clear", "create", "update", "delete"],
					description: "操作：list 列表 / show 当前角色 / set 激活 / clear 清除 / create 创建 / update 修改 / delete 删除",
				},
				id: { type: "string", description: "角色 id（set/create/update/delete 用；1~48 字符小写字母数字中划线）" },
				name: { type: "string", description: "角色显示名（create/update 用；1~64 字符）" },
				prompt: { type: "string", description: "角色指令文本（create/update 用；1~2000 字符，注入系统提示）" },
			},
			required: ["op"],
		},

		async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, _ctx: any) {
			try {
				const { op, id, name, prompt } = params ?? {};
				if (typeof op !== "string" || op.length === 0) {
					return {
						content: [{ type: "text", text: "role 需要 op 参数（list/show/set/clear/create/update/delete）" }],
						details: { error: "missing op" },
					};
				}
				const result = await runRole(op, id, name, prompt);
				if (result.error) {
					return { content: [{ type: "text", text: result.error }], details: { error: result.error } };
				}
				return {
					content: [{ type: "text", text: result.text }],
					details: result.details ?? {},
				};
			} catch (e: any) {
				const message = `[role] 操作失败: ${String(e?.message ?? e)}`;
				console.error(message);
				return { content: [{ type: "text", text: message }], details: { error: message } };
			}
		},
	});
}
