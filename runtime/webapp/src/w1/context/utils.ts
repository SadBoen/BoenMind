//! 上下文页工具与类型(自 context.tsx 机械移入)。

// 诚实原则:模型上下文窗口容量不做任何猜测——唯一数据源是用户在
// 「设置 → 模型提供商」为模型登记的窗口值(model.json contextWindows);
// 未登记就显示「未知」,绝不用名字匹配表冒充真实水位。

export const fmtDur = (ms?: number | null) =>
  ms == null ? "—" : ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;

export const estTokens = (s?: string | null) => Math.max(1, Math.ceil((s?.length ?? 0) / 3));

/** 双栏联动通用平滑滚动定位(#22 拆分:自 context.tsx 外置,各 Tab 共用) */
export const scrollToId = (id: string) => {
  const el = document.getElementById(id);
  if (el) {
    el.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }
};


// 被本轮操作影响的本地文件记录 (对标 Pi-Web File Tracking)
export interface FileSideEffect {
  path: string;
  action: "read" | "write" | "edit" | "exec";
  toolName: string;
  detail: string;
  /** 新增行数 (write=整文行数; edit=old_string/new_string 差量统计) */
  linesAdded?: number;
  /** 删除行数 (edit 替换掉的行数) */
  linesRemoved?: number;
}

// 对 Prompt 的 system 内容进行结构拆解 (人设/技能/工作区)
export interface ParsedPromptRecipe {
  rawSystemPrompt: string;
  personaText: string;
  skills: Array<{ id: string; name: string; instruction: string }>;
  workspaceText: string | null;
  historyTurns: Array<{ turnIndex: number; user: string; assistant: string }>;
  currentUserInput: string;
  toolList: Array<{
    name: string;
    description: string;
    needsApproval: boolean;
    paramTokens: number;
    rawSchema: any;
  }>;
  affectedFiles: FileSideEffect[];
  reasoningSnippet: string | null;
}
