import { useState } from "react";
import {
  Search,
  ChevronDown,
  ChevronRight,
  Pencil,
  CheckCircle2,
  FolderGit2,
  Maximize2,
  Minimize2,
} from "lucide-react";
import { ExploreGroupBlock, ChangesGroupBlock, ToolActionItem } from "../parser";
import { FileBadge } from "./FileBadge";

interface ToolTreeGroupProps {
  block: ExploreGroupBlock | ChangesGroupBlock | { type: "generic_tool_group"; items: ToolActionItem[] };
}

/**
 * 行为树状合并组件:
 * 1. 连续只读（查阅/搜索）合并为母卡片「查阅 · N 文件」或「探索 · N 搜索」
 * 2. 连续文件修改合并为「更改了 N 个文件」，后排彩色文件胶囊
 * 3. 展开呈现类似目录树的左侧垂直细引导线
 * 4. 每个子项拥有独立的状态和彩色文件图标，支持展开限高与无滚动条查看
 */
export function ToolTreeGroup({ block }: ToolTreeGroupProps) {
  const [open, setOpen] = useState(false);

  // 1. 查阅类聚合 (ExploreGroup)
  if (block.type === "explore_group") {
    const { items, fileCount, searchCount } = block;
    const summaryParts: string[] = [];
    if (fileCount > 0) summaryParts.push(`${fileCount} 文件`);
    if (searchCount > 0) summaryParts.push(`${searchCount} 搜索`);
    const titleText = summaryParts.length > 0 ? summaryParts.join("，") : `${items.length} 个操作`;

    return (
      <div className="rounded-lg border border-border/40 bg-card/60 overflow-hidden transition-all duration-200">
        {/* 母卡片头部 */}
        <div
          className="flex items-center gap-2 px-2.5 py-1 cursor-pointer select-none text-xs hover:bg-muted/40 transition-colors"
          onClick={() => setOpen(!open)}
        >
          {open ? <ChevronDown size={13} className="shrink-0 text-muted-foreground" /> : <ChevronRight size={13} className="shrink-0 text-muted-foreground" />}
          <Search className="size-3.5 shrink-0 text-sky-500" />
          <span className="font-semibold text-foreground/90">查阅 · {titleText}</span>
          <span className="text-[11px] text-muted-foreground/60 ml-auto">{open ? "收起" : "展开树"}</span>
        </div>

        {/* 树状子项列表 */}
        {open ? (
          <div className="border-t border-dashed border-border/60 bg-muted/20 px-3 py-2">
            <div className="ml-2 pl-3.5 border-l border-border/60 space-y-1">
              {items.map((it, idx) => (
                <TreeSubItem key={idx} item={it} />
              ))}
            </div>
          </div>
        ) : null}
      </div>
    );
  }

  // 2. 改动类聚合 (ChangesGroup)
  if (block.type === "changes_group") {
    const { items, fileCount } = block;
    return (
      <div className="rounded-lg border border-border/40 bg-card/60 overflow-hidden transition-all duration-200">
        <div
          className="flex items-center gap-2 px-2.5 py-1 cursor-pointer select-none text-xs hover:bg-muted/40 transition-colors"
          onClick={() => setOpen(!open)}
        >
          {open ? <ChevronDown size={13} className="shrink-0 text-muted-foreground" /> : <ChevronRight size={13} className="shrink-0 text-muted-foreground" />}
          <Pencil className="size-3.5 shrink-0 text-emerald-500" />
          <span className="font-semibold text-foreground/90">更改了 {fileCount} 个文件</span>
          {/* 未展开时在右侧直接预览彩色文件徽章 */}
          {!open ? (
            <div className="flex items-center gap-1.5 overflow-hidden ml-1 truncate">
              {items.slice(0, 3).map((it, idx) => (
                <FileBadge key={idx} path={it.target || it.name} />
              ))}
              {items.length > 3 ? (
                <span className="text-[10px] text-muted-foreground">+{items.length - 3}</span>
              ) : null}
            </div>
          ) : null}
          <span className="text-[11px] text-muted-foreground/60 ml-auto">{open ? "收起" : "展开详情"}</span>
        </div>

        {open ? (
          <div className="border-t border-dashed border-border/60 bg-muted/20 px-3 py-2">
            <div className="ml-2 pl-3.5 border-l border-border/60 space-y-1">
              {items.map((it, idx) => (
                <div key={idx} className="flex items-center justify-between text-xs py-1">
                  <div className="flex items-center gap-2 min-w-0">
                    <FileBadge path={it.target || it.name} />
                    <span className="text-muted-foreground text-[11px] font-mono">{it.name}</span>
                  </div>
                  <span className="text-emerald-500 text-[11px] flex items-center gap-1">
                    <CheckCircle2 size={11} /> 已应用
                  </span>
                </div>
              ))}
            </div>
          </div>
        ) : null}
      </div>
    );
  }

  // 3. 通用工具聚合
  return (
    <div className="rounded-lg border border-border/40 bg-card/60 overflow-hidden">
      <div
        className="flex items-center gap-2 px-2.5 py-1 cursor-pointer text-xs hover:bg-muted/40"
        onClick={() => setOpen(!open)}
      >
        {open ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
        <FolderGit2 className="size-3.5 text-indigo-500" />
        <span className="font-semibold text-foreground/90">{block.items.length} 个操作</span>
        <span className="text-[11px] text-muted-foreground/60 ml-auto">{open ? "收起" : "展开"}</span>
      </div>
      {open ? (
        <div className="border-t border-dashed border-border/60 bg-muted/20 p-2 space-y-1">
          {block.items.map((it, idx) => (
            <TreeSubItem key={idx} item={it} />
          ))}
        </div>
      ) : null}
    </div>
  );
}

// 树形子节点单元
function TreeSubItem({ item }: { item: ToolActionItem }) {
  const [subOpen, setSubOpen] = useState(false);
  const [fullyExpanded, setFullyExpanded] = useState(false);
  const isRead = item.category === "read";
  const target = item.target || "";

  return (
    <div className="group relative">
      <div
        className="flex items-center gap-2 py-1 text-xs cursor-pointer select-none rounded hover:bg-muted/40 px-1 -ml-1 transition-colors"
        onClick={() => setSubOpen(!subOpen)}
      >
        {subOpen ? <ChevronDown size={12} className="text-muted-foreground shrink-0" /> : <ChevronRight size={12} className="text-muted-foreground shrink-0" />}
        {isRead ? (
          <FileBadge path={target || item.name} />
        ) : (
          <div className="flex items-center gap-1.5 font-mono text-[11.5px] truncate flex-1 min-w-0">
            <Search size={12} className="text-sky-500 shrink-0" />
            <span className="font-medium text-foreground/90 shrink-0">{item.name}</span>
            {target ? <span className="text-muted-foreground bg-muted/50 px-1.5 py-0.5 rounded text-[11px] truncate">{target}</span> : null}
          </div>
        )}
        {item.elapsedMs ? (
          <span className="text-[10.5px] font-mono text-muted-foreground/80 shrink-0">
            {item.elapsedMs}ms
          </span>
        ) : null}
        <span className="text-[10px] text-muted-foreground/60 ml-auto opacity-0 group-hover:opacity-100 transition-opacity">
          {subOpen ? "收起" : "详情"}
        </span>
      </div>

      {subOpen ? (
        <div className="mt-1 pl-4 text-[11.5px] text-muted-foreground font-mono bg-muted/30 p-2 rounded border border-border/30">
          <div className="flex items-center justify-between pb-1 text-[10px]">
            <span>操作入参 / 目标</span>
            <button
              onClick={() => setFullyExpanded(!fullyExpanded)}
              className="hover:text-foreground flex items-center gap-0.5"
            >
              {fullyExpanded ? <Minimize2 size={10} /> : <Maximize2 size={10} />}
            </button>
          </div>
          <div className={`overflow-y-auto no-scrollbar ${fullyExpanded ? "max-h-60" : "max-h-[3.8rem]"}`}>
            <span className="text-foreground/80 break-all">{target || item.name}</span>
          </div>
        </div>
      ) : null}
    </div>
  );
}
