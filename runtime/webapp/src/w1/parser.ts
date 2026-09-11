// 任务执行过程结构化解析器:将模型输出文本拆解为思考流、工具调用与 Markdown 正文
// 借鉴 ZCode / DSH 的行为收敛思想，实现流式思考切分与连续同类操作二级聚合

export type ToolActionItem = {
  raw: string;
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

// 工具名称到大类的映射(includes 即覆盖 fs.read/fs.search/fs.write/fs.edit/
// system.exec 前缀——前缀名含关键词,无需另立 startsWith 分支)
export function classifyTool(toolName: string): ToolActionItem["category"] {
  const lower = toolName.toLowerCase();
  if (lower.includes("read")) {
    return "read";
  }
  if (lower.includes("search") || lower.includes("grep") || lower.includes("find")) {
    return "search";
  }
  if (lower.includes("write") || lower.includes("edit")) {
    return "edit";
  }
  if (lower.includes("exec") || lower.includes("bash") || lower.includes("terminal") || lower.includes("powershell")) {
    return "exec";
  }
  return "other";
}

/**
 * 解析模型消息正文，拆解思考、工具与普通文本，并进行同类树状聚合
 */
export function parseAssistantContent(raw: string, isMessageRunning: boolean = false): ParsedContentBlock[] {
  if (!raw) return [];

  const rawBlocks: Array<
    | { type: "text"; text: string }
    | { type: "thinking"; text: string; isStreaming?: boolean; elapsedSeconds?: number }
    | { type: "tool_item"; item: ToolActionItem }
  > = [];

  let cursor = 0;
  const thinkStartRegex = /<think(?:ing)?>/gi;
  const thinkEndRegex = /<\/think(?:ing)?>/gi;

  // 1. 扫描与提取思考链块 (<think> ... </think> 或未闭合的流式思考)
  while (cursor < raw.length) {
    thinkStartRegex.lastIndex = cursor;
    const startMatch = thinkStartRegex.exec(raw);
    if (!startMatch) {
      // 后面没有思考标签，全部进入普通处理
      processTextAndTools(raw.slice(cursor), rawBlocks);
      break;
    }

    // 思考标签之前有普通文本/工具
    if (startMatch.index > cursor) {
      processTextAndTools(raw.slice(cursor, startMatch.index), rawBlocks);
    }

    // 寻找思考结束标签
    const thinkContentStart = startMatch.index + startMatch[0].length;
    thinkEndRegex.lastIndex = thinkContentStart;
    const endMatch = thinkEndRegex.exec(raw);

    if (endMatch) {
      const thinkText = raw.slice(thinkContentStart, endMatch.index).trim();
      if (thinkText) {
        rawBlocks.push({
          type: "thinking",
          text: thinkText,
          isStreaming: false,
        });
      }
      cursor = endMatch.index + endMatch[0].length;
    } else {
      // 未闭合思考标签（流式进行中）
      const thinkText = raw.slice(thinkContentStart).trim();
      rawBlocks.push({
        type: "thinking",
        text: thinkText,
        isStreaming: isMessageRunning,
      });
      break;
    }
  }

  // 2. 二级聚合：合并连续的同类工具行为（ZCode 核心模式）
  return aggregateToolBlocks(rawBlocks);
}

// 辅助：从非思考文本中按行提取工具标记与正文
function processTextAndTools(
  chunk: string,
  out: Array<
    | { type: "text"; text: string }
    | { type: "thinking"; text: string; isStreaming?: boolean; elapsedSeconds?: number }
    | { type: "tool_item"; item: ToolActionItem }
  >,
) {
  if (!chunk) return;
  const lines = chunk.split("\n");
  let curText = "";

  const flushText = () => {
    if (curText) {
      out.push({ type: "text", text: curText });
      curText = "";
    }
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    // 匹配 [调用 tool_name target]
    const toolMatch = line.match(/^\[调用\s+([a-zA-Z0-9_.:-]+)(?:\s*(.*?))?\]$/);
    // 匹配 [工具完成 tool_name 耗时 Xms]
    const doneMatch = line.match(/^\[工具完成\s+([a-zA-Z0-9_.:-]+)(?:\s*耗时\s*(\d+)ms)?\]$/);

    if (toolMatch) {
      flushText();
      const name = toolMatch[1];
      const target = (toolMatch[2] || "").trim();
      const category = classifyTool(name);
      out.push({
        type: "tool_item",
        item: {
          raw: line,
          name,
          target: target || undefined,
          category,
          status: "done",
        },
      });
    } else if (doneMatch) {
      flushText();
      const name = doneMatch[1];
      const ms = doneMatch[2] ? parseInt(doneMatch[2], 10) : undefined;
      // 回填上一个匹配项的耗时
      for (let j = out.length - 1; j >= 0; j--) {
        const item = out[j];
        if (item.type === "tool_item" && item.item.name === name) {
          item.item.elapsedMs = ms;
          break;
        }
      }
    } else {
      curText += (curText ? "\n" : "") + line;
    }
  }

  flushText();
}

// 二级聚合管道：连续只读行为（read/search）合并为 ExploreGroup，连续编辑（edit）合并为 ChangesGroup
function aggregateToolBlocks(
  blocks: Array<
    | { type: "text"; text: string }
    | { type: "thinking"; text: string; isStreaming?: boolean; elapsedSeconds?: number }
    | { type: "tool_item"; item: ToolActionItem }
  >,
): ParsedContentBlock[] {
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

  for (const block of blocks) {
    if (block.type === "tool_item") {
      const cat = block.item.category;
      if (cat === "read" || cat === "search") {
        flushChanges();
        pendingExplore.push(block.item);
      } else if (cat === "edit") {
        flushExplore();
        pendingChanges.push(block.item);
      } else if (cat === "exec") {
        flushExplore();
        flushChanges();
        result.push({
          type: "terminal_block",
          item: block.item,
        });
      } else {
        flushExplore();
        flushChanges();
        result.push({
          type: "generic_tool_group",
          items: [block.item],
        });
      }
    } else {
      flushExplore();
      flushChanges();
      result.push(block);
    }
  }

  flushExplore();
  flushChanges();

  return result;
}
