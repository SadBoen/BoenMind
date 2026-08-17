/**
 * 工具调用摘要规则库（2026-08-17，借鉴 hermes-agent 的动词短语表 + DSH 的
 * SUMMARY_KEYS 偏好键提取；hermes 的架构教训：wire 上只发参数预览，
 * 动词短语由客户端自己拼 —— 本模块即 BoenMind 前端的"客户端措辞层"）。
 *
 * 目标：工具执行过程一扫而过能看懂（如"运行命令 npm test"），
 * 原始参数 JSON 折叠进展开区，不再把大段 JSON 平铺在消息流里。
 *
 * 未知工具兜底：`调用工具 · 工具名 · 首个字符串参数`，不中断阅读。
 */
import type { LucideIcon } from "lucide-react";
import {
  AlarmClock,
  CheckSquare,
  FileEdit,
  FileText,
  Folder,
  GitBranch,
  Globe,
  HelpCircle,
  Minimize2,
  Search,
  Terminal,
  Wrench,
  BrainCircuit,
} from "lucide-react";

export type ToolDisplayMode = "auto" | "full" | "summary" | "compact";

interface ToolRule {
  /** 已登记（false = 通用兜底） */
  known?: boolean;
  /** 动词翻译 key（i18n: chat.tool.verb.<verb>） */
  verb: string;
  /** 主参数键（按偏好顺序取第一个存在的；支持嵌套路径 key "a.b"） */
  args: string[];
  /** 未取到主参数时尝试取的次选键 */
  altArgs?: string[];
  /** 无主参数可显示时，显示"N 个参数"（todo/tasks 等批量工具） */
  countFallback?: boolean;
  icon: LucideIcon;
}

/** 工具名 → 显示规则（新增工具在此登记；规则缺失走通用兜底） */
export const TOOL_RULES: Record<string, ToolRule> = {
  // 终端/命令执行
  bash: { verb: "runCommand", args: ["command"], altArgs: ["cmd"], icon: Terminal },
  exec: { verb: "runCommand", args: ["command"], altArgs: ["cmd"], icon: Terminal },
  terminal: { verb: "runCommand", args: ["command"], altArgs: ["cmd"], icon: Terminal },
  // 文件读取/浏览
  read_file: { verb: "readFile", args: ["path"], altArgs: ["file_path", "file"], icon: FileText },
  read: { verb: "readFile", args: ["path"], altArgs: ["file_path", "file"], icon: FileText },
  ls: { verb: "listDir", args: ["path"], altArgs: ["directory"], icon: Folder },
  list_dir: { verb: "listDir", args: ["path"], altArgs: ["directory"], icon: Folder },
  glob: { verb: "findFiles", args: ["pattern"], altArgs: ["glob", "path"], icon: Folder },
  find: { verb: "findFiles", args: ["pattern"], altArgs: ["path"], icon: Folder },  // 文件编辑/写入
  write_file: { verb: "writeFile", args: ["path"], altArgs: ["file_path", "file"], icon: FileEdit },
  write: { verb: "writeFile", args: ["path"], altArgs: ["file_path", "file"], icon: FileEdit },
  edit_file: { verb: "editFile", args: ["path"], altArgs: ["file_path", "file"], icon: FileEdit },
  edit: { verb: "editFile", args: ["path"], altArgs: ["file_path", "file"], icon: FileEdit },
  create_file: { verb: "writeFile", args: ["path"], altArgs: ["file_path"], icon: FileEdit },
  delete_file: { verb: "deleteFile", args: ["path"], altArgs: ["file_path"], icon: FileEdit },
  // 搜索
  grep: { verb: "searchIn", args: ["pattern"], altArgs: ["path", "directory"], icon: Search },
  rg: { verb: "searchIn", args: ["pattern"], altArgs: ["path"], icon: Search },
  // 网络
  web_search: { verb: "searchWeb", args: ["query"], altArgs: ["q"], icon: Globe },
  web_fetch: { verb: "fetchUrl", args: ["url"], altArgs: ["link"], icon: Globe },
  web_scrape: { verb: "scrapePage", args: ["url"], altArgs: ["link"], icon: Globe },
  // 记忆
  memory: { verb: "updateMemory", args: ["target", "key"], altArgs: ["content"], icon: BrainCircuit },
  remember: { verb: "updateMemory", args: ["key"], altArgs: ["target"], icon: BrainCircuit },
  // 任务清单
  todo: { verb: "updateTodos", args: [], countFallback: true, icon: CheckSquare },
  todos: { verb: "updateTodos", args: [], countFallback: true, icon: CheckSquare },
  tasks: { verb: "updateTodos", args: [], countFallback: true, icon: CheckSquare },
  // 子代理/派工
  subagent: { verb: "spawnSubagent", args: ["task"], altArgs: ["goal", "prompt"], icon: GitBranch },
  delegate_task: { verb: "spawnSubagent", args: ["task"], altArgs: ["goal"], icon: GitBranch },
  // 系统
  compactor: { verb: "compactContext", args: ["reason"], icon: Minimize2 },
  ctx_compact: { verb: "compactContext", args: ["reason"], icon: Minimize2 },
  set_wake: { verb: "setWake", args: ["interval"], icon: AlarmClock },
  ask_user: { verb: "askUser", args: ["question"], icon: HelpCircle },
  ask: { verb: "askUser", args: ["question"], icon: HelpCircle },
};

/** 通用兜底规则（未知工具） */
const FALLBACK_RULE: ToolRule = { known: false, verb: "unknown", args: [], icon: Wrench };

/** 取嵌套 key（"a.b"）与偏好顺序首个存在的值；value 需为字符串/数字 */
function pickArg(args: unknown, keys: string[]): string | undefined {
  if (!args || typeof args !== "object") return undefined;
  const obj = args as Record<string, unknown>;
  for (const key of keys) {
    const val = key.includes(".") ? key.split(".").reduce<unknown>((o, k) => (o && typeof o === "object" ? (o as Record<string, unknown>)[k] : undefined), obj) : obj[key];
    if (typeof val === "string" && val.trim()) return val.trim();
    if (typeof val === "number") return String(val);
    if (Array.isArray(val) && val.length > 0 && typeof val[0] === "string" && val[0].trim()) {
      // 数组（如 query 多关键词）取前两个拼接
      const head = val.slice(0, 2).join(" ");
      return head.length > 80 ? `${head.slice(0, 80)}…` : head;
    }
  }
  return undefined;
}

/** 单行截断（摘要行用，避免长命令/长路径撑爆消息流） */
function clamp(s: string, max = 60): string {
  const oneLine = s.replace(/\s+/g, " ").trim();
  return oneLine.length > max ? `${oneLine.slice(0, max)}…` : oneLine;
}

export interface ToolSummary {
  /** 动词短语标签（已 i18n 拼好，如"运行命令 npm test"） */
  label: string;
  /** 主参数预览（未知工具 = 首个字符串参数；无 = 空串） */
  preview: string;
  /** 规则是否存在（false = 未知工具，label 为兜底短语） */
  known: boolean;
  /** 分类图标（左侧图标 + 标题栏共用） */
  icon: LucideIcon;
}

/**
 * 生成工具调用摘要（i18n 动词由外部传入翻译函数，避免本模块依赖 i18next）。
 * @param toolName  工具名（snake_case）
 * @param args      原始参数（任意 JSON）
 * @param verbOf    动词翻译函数（(key) => 本地化动词短语，如 "运行命令"）
 */
export function summarizeToolCall(
  toolName: string,
  args: unknown,
  verbOf: (key: string) => string,
): ToolSummary {
  const rule = TOOL_RULES[toolName] ?? FALLBACK_RULE;
  const known = rule.known !== false;
  const verb = verbOf(`chat.tool.verb.${rule.verb}`);
  const primary = pickArg(args, rule.args) ?? pickArg(args, rule.altArgs ?? []);

  if (!known) {
    // 未知工具：标签 = 兜底动词 + 工具名，预览 = 首个字符串参数（若有）
    return { label: `${verb} ${toolName}`, preview: clamp(primary ?? ""), known: false, icon: rule.icon };
  }
  if (primary) {
    return { label: verb, preview: clamp(primary), known: true, icon: rule.icon };
  }
  if (rule.countFallback) {
    // 批量工具：显示条目数（todo/tasks 传数组）
    const n = args && typeof args === "object"
      ? (Object.values(args as Record<string, unknown>).find((v) => Array.isArray(v)) as unknown[] | undefined)?.length
      : undefined;
    return { label: verb, preview: n ? `(${n})` : "", known: true, icon: rule.icon };
  }
  return { label: verb, preview: "", known: true, icon: rule.icon };
}
