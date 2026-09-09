import { useState } from "react";
import { Search, Pencil, Plug, ChevronDown, ChevronRight } from "lucide-react";
import { ExploreGroupBlock, ChangesGroupBlock, ToolActionItem } from "../parser";

interface ToolTreeGroupProps {
  block: ExploreGroupBlock | ChangesGroupBlock | { type: "generic_tool_group"; items: ToolActionItem[] };
}

/**
 * 行为树状合并行(2026-09-10 对齐 ZCode 样式,废除卡片/彩色徽章/树形子节点):
 * 1. 连续只读聚合为灰字行「查阅 · N 搜索, N 文件」(口径与 ZCode 一致:搜索在前)
 * 2. 连续文件修改聚合为「更改了 N 个文件」
 * 3. 其余工具聚为「N 个操作」
 * 展开内容统一挂 2px 左细线,子项为通栏灰字单行,超宽截断;悬停 title 看全量。
 * 蓝本 token 见 .zcode/zcode-chat-replica.html(离线复刻留档)
 */
export function ToolTreeGroup({ block }: ToolTreeGroupProps) {
  const [open, setOpen] = useState(false);

  // 1. 查阅类聚合 (ExploreGroup)
  if (block.type === "explore_group") {
    const { items, fileCount, searchCount } = block;
    const parts: string[] = [];
    if (searchCount > 0) parts.push(`${searchCount} 搜索`);
    if (fileCount > 0) parts.push(`${fileCount} 文件`);
    const titleText = parts.length > 0 ? parts.join(", ") : `${items.length} 个操作`;

    return (
      <div>
        <div
          className="zc-t clickable"
          data-slot="tool-group-head"
          onClick={() => setOpen(!open)}
        >
          <Search />
          <span>查阅 · {titleText}</span>
          {open ? (
            <ChevronDown className="zc-chev open" />
          ) : (
            <ChevronRight className="zc-chev" />
          )}
        </div>
        {open ? (
          <div className="zc-sub">
            {items.map((it, idx) => (
              <div
                key={idx}
                className="zc-subitem"
                title={it.target || it.name}
              >
                {it.target || it.name}
              </div>
            ))}
          </div>
        ) : null}
      </div>
    );
  }

  // 2. 改动类聚合 (ChangesGroup)
  if (block.type === "changes_group") {
    const { items, fileCount } = block;
    return (
      <div>
        <div
          className="zc-t clickable"
          data-slot="changes-group-head"
          onClick={() => setOpen(!open)}
        >
          <Pencil />
          <span>更改了 {fileCount} 个文件</span>
          {open ? (
            <ChevronDown className="zc-chev open" />
          ) : (
            <ChevronRight className="zc-chev" />
          )}
        </div>
        {open ? (
          <div className="zc-sub">
            {items.map((it, idx) => (
              <div
                key={idx}
                className="zc-subitem"
                title={it.target || it.name}
              >
                {it.target || it.name}
              </div>
            ))}
          </div>
        ) : null}
      </div>
    );
  }

  // 3. 通用工具聚合
  return (
    <div>
      <div
        className="zc-t clickable"
        data-slot="generic-tool-head"
        onClick={() => setOpen(!open)}
      >
        <Plug />
        <span>{block.items.length} 个操作</span>
        {open ? (
          <ChevronDown className="zc-chev open" />
        ) : (
          <ChevronRight className="zc-chev" />
        )}
      </div>
      {open ? (
        <div className="zc-sub">
          {block.items.map((it, idx) => (
            <div
              key={idx}
              className="zc-subitem"
              title={it.target || it.name}
            >
              {it.target || it.name}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}
