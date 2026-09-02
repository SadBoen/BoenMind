"use client";
// 原型 = assistant-ui 注册表 elements-file-tree(2026-09-01 选装,MIT)。
// W2 适配:保留原型行形态(缩进 depth / mono / 图标 / 悬停),交互自有——
// 目录懒加载展开 + 点击(目录=展开收起,文件=回调打开预览);去 diff 统计头。
import type { ComponentProps } from "react";
import {
  ChevronDownIcon,
  ChevronRightIcon,
  FileIcon,
  FolderIcon,
  Loader2Icon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { mono } from "@/lib/surfaces";

export interface FileTreeNode {
  path: string;
  name: string;
  depth: number;
  kind: "folder" | "file";
  expanded?: boolean;
  loading?: boolean;
}

export function FileTree({
  nodes,
  onNodeClick,
  onNodeContextMenu,
  className,
  ...props
}: Omit<ComponentProps<"div">, "children" | "nodes"> & {
  nodes: readonly FileTreeNode[];
  onNodeClick?: (node: FileTreeNode) => void;
  onNodeContextMenu?: (node: FileTreeNode, pos: { x: number; y: number }) => void;
}) {
  // 用户裁定:不做独立底色卡片,融入所在面板(透明、无边框)
  return (
    <div
      data-slot="file-tree"
      className={cn("flex w-full flex-col gap-1 p-1", className)}
      {...props}
    >
      <div className="flex flex-col">
        {nodes.map((node) => (
          <button
            key={node.path}
            type="button"
            data-path={node.path}
            onClick={() => onNodeClick?.(node)}
            onContextMenu={(e) => {
              if (!onNodeContextMenu) return;
              e.preventDefault();
              onNodeContextMenu(node, { x: e.clientX, y: e.clientY });
            }}
            className="hover:bg-foreground/[0.05] flex items-center gap-2 rounded-lg px-1 py-1 text-left text-[13px] transition-colors duration-150"
            style={{ paddingInlineStart: `${0.25 + node.depth * 0.85}rem` }}
          >
            {node.kind === "folder" ? (
              <>
                {node.expanded ? (
                  <ChevronDownIcon className="text-foreground/25 size-3 shrink-0" />
                ) : (
                  <ChevronRightIcon className="text-foreground/25 size-3 shrink-0" />
                )}
                <FolderIcon className="text-foreground/35 size-3.5 shrink-0" />
                <span className="text-foreground/70 min-w-0 flex-1 truncate">
                  {node.name}
                </span>
                {node.loading ? (
                  <Loader2Icon
                    data-slot="spinner"
                    className="text-foreground/30 size-3 shrink-0 animate-spin"
                  />
                ) : null}
              </>
            ) : (
              <>
                <FileIcon className="text-foreground/30 ms-4 size-3.5 shrink-0" />
                <span className="text-foreground/85 min-w-0 flex-1 truncate">
                  {node.name}
                </span>
              </>
            )}
          </button>
        ))}
        {nodes.length === 0 ? (
          <div className={cn(mono, "text-foreground/30 px-2 py-3")}>
            空目录
          </div>
        ) : null}
      </div>
    </div>
  );
}
