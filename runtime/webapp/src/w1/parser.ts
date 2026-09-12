// 任务执行过程结构化解析器:将助手正文拆解为思考流与 Markdown 正文;
// 工具调用不再从正文文本解析——ADR-0055 起由后端 bm_event 结构化帧直送,
// 见 toolBlocksFromEvents。分类据 toolEvent.effect(后端 manifest 风险声明),
// 不再按工具名子串猜。

import type { ToolEvent } from "./runtime";

export type ToolActionItem = {
  name: string;
  target?: string;
  category: "read" | "search" | "edit" | "exec" | "other";
  status?: "running" | "done";
  elapsedMs?: number;
};

export type ExploreGroupBlock = {
  type: "explore_group";
  items: ToolActionItem[];
  fileCount: number;
  searchCount: number;
};

export type ChangesGroupBlock = {
  type: "changes_group";
  items: ToolActionItem[];
  fileCount: number;
};

export type TerminalBlockItem = {
  type: "terminal_block";
  item: ToolActionItem;
};

export type ParsedContentBlock =
  | { type: "text"; text: string }
  | { type: "thinking"; text: string; isStreaming?: boolean; elapsedSeconds?: number }
  | ExploreGroupBlock
  | ChangesGroupBlock
  | TerminalBlockItem
  | { type: "generic_tool_group"; items: ToolActionItem[] };

/**
 * 工具事件 → 展示分组类别。据后端声明的 effect(风险等级)分档,仅在
 * read-only 内用**精确能力名**区分「搜索」与「读文件」(= fs.search);
 * system.exec 单独归「终端」。均为展示分类,不再对任意工具名做子串猜测。
 */
export function categoryOfToolEvent(t: ToolEvent): ToolActionItem["category"] {
  if (t.capability === "system.exec") return "exec";
  if (t.effect === "read-only") {
    return t.capability === "fs.search" ? "search" : "read";
  }
  if (t.effect === "" && t.capability === "") return "other";
  // 变更类(reversible/external/high-risk):文件写入/编辑或其他副作用工具
  return "edit";
}

/** 工具事件 → 展示项(供分组聚合) */
export function toolItemOf(t: ToolEvent): ToolActionItem {
  return {
    name: t.capability,
    target: t.target || undefined,
    category: categoryOfToolEvent(t),
    status: t.status,
    elapsedMs: t.elapsedMs,
  };
}

/**
 * 结构化工具事件 → 聚合块(与正文文本无关;ADR-0055)。
 * 二级聚合:连续只读(read/search)→ ExploreGroup;连续变更(edit)→ ChangesGroup;
 * exec 单独成 TerminalBlock;其余 → generic_tool_group。
 */
export function toolBlocksFromEvents(events: ToolEvent[]): ParsedContentBlock[] {
  const items = events.map(toolItemOf);
  const result: ParsedContentBlock[] = [];
  let pendingExplore: ToolActionItem[] = [];
  let pendingChanges: ToolActionItem[] = [];

  const flushExplore = () => {
    if (pendingExplore.length > 0) {
      if (pendingExplore.length === 1 && pendingExplore[0].category === "other") {
        result.push({ type: "generic_tool_group", items: [...pendingExplore] });
      } else {
        const fileCount = pendingExplore.filter((it) => it.category === "read").length;
        const searchCount = pendingExplore.filter((it) => it.category === "search").length;
        result.push({
          type: "explore_group",
          items: [...pendingExplore],
          fileCount,
          searchCount,
        });
      }
      pendingExplore = [];
    }
  };

  const flushChanges = () => {
    if (pendingChanges.length > 0) {
      result.push({
        type: "changes_group",
        items: [...pendingChanges],
        fileCount: pendingChanges.length,
      });
      pendingChanges = [];
    }
  };

  for (const item of items) {
    const cat = item.category;
    if (cat === "read" || cat === "search") {
      flushChanges();
      pendingExplore.push(item);
    } else if (cat === "edit") {
      flushExplore();
      pendingChanges.push(item);
    } else if (cat === "exec") {
      flushExplore();
      flushChanges();
      result.push({ type: "terminal_block", item });
    } else {
      flushExplore();
      flushChanges();
      result.push({ type: "generic_tool_group", items: [item] });
    }
  }

  flushExplore();
  flushChanges();
  return result;
}

/**
 * 解析助手正文:仅拆解思考链与普通文本(工具已改走结构化事件,不再解析
 * 正文里的 `[调用 …]`/`[工具完成 …]` 文本标记)。
 */
export function parseAssistantContent(
  raw: string,
  isMessageRunning: boolean = false,
): ParsedContentBlock[] {
  if (!raw) return [];

  const blocks: ParsedContentBlock[] = [];
  let cursor = 0;
  const thinkStartRegex = /<think(?:ing)?>/gi;
  const thinkEndRegex = /<\/think(?:ing)?>/gi;

  while (cursor < raw.length) {
    thinkStartRegex.lastIndex = cursor;
    const startMatch = thinkStartRegex.exec(raw);
    if (!startMatch) {
      if (cursor < raw.length) blocks.push({ type: "text", text: raw.slice(cursor) });
      break;
    }
    if (startMatch.index > cursor) {
      blocks.push({ type: "text", text: raw.slice(cursor, startMatch.index) });
    }
    const thinkContentStart = startMatch.index + startMatch[0].length;
    thinkEndRegex.lastIndex = thinkContentStart;
    const endMatch = thinkEndRegex.exec(raw);
    if (endMatch) {
      const thinkText = raw.slice(thinkContentStart, endMatch.index).trim();
      if (thinkText) {
        blocks.push({ type: "thinking", text: thinkText, isStreaming: false });
      }
      cursor = endMatch.index + endMatch[0].length;
    } else {
      const thinkText = raw.slice(thinkContentStart).trim();
      if (thinkText) {
        blocks.push({ type: "thinking", text: thinkText, isStreaming: isMessageRunning });
      }
      break;
    }
  }
  return blocks;
}
