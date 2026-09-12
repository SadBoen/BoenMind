"use client";
// 原型 = assistant-ui 注册表 elements-file-tree()。
// W2 适配:保留原型行形态(缩进 depth / mono / 图标 / 悬停),交互自有。
// flattenTree 拍平(工作区文件树 / 设置页全盘目录选择器同源),
// FileTree 增 selected 高亮 / drive 图标 / 行尾徽标 / chevron 独立点击区。
import { useCallback, useEffect, useRef, useState } from "react";
import type { ComponentProps, ReactNode } from "react";
import {
  ChevronDownIcon,
  ChevronRightIcon,
  FileIcon,
  FolderIcon,
  HardDriveIcon,
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
  selected?: boolean;
  drive?: boolean;
}

// 懒加载树条目:消费方把 API 响应映射成这个形状(path 即树 key,
// 文件树用工作区相对路径,选择器用全盘绝对路径)
export interface LazyTreeEntry {
  path: string;
  name: string;
  kind: "folder" | "file";
  drive?: boolean;
}

/**
 * 懒加载目录树状态:children 缓存 + 展开态 + 逐级展开定位。
 * load 失败由消费方自行兜底(捕获后返回 [] 并自行报错)。
 */
export function useLazyTree(opts: {
  load: (path: string) => Promise<LazyTreeEntry[]>;
  initialExpanded?: string[];
}) {
  const [children, setChildren] = useState<Record<string, LazyTreeEntry[]>>({});
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(
    new Set(opts.initialExpanded ?? []),
  );
  const [loadingDir, setLoadingDir] = useState<string | null>(null);
  const childrenRef = useRef(children);
 // 镜像同步放 effect 中(React Compiler 规则:渲染期不可写 ref;事件处理器
 // toggle/reveal 读到的 childrenRef 需是最新 children,effect 同步即可)
  useEffect(() => {
    childrenRef.current = children;
  }, [children]);

  const loadDir = useCallback(
    async (path: string) => {
      setLoadingDir(path);
      try {
        const entries = await opts.load(path);
        setChildren((cur) => ({ ...cur, [path]: entries }));
      } finally {
        setLoadingDir(null);
      }
    },
    [opts.load],
  );

 // 展开/收起;未加载过则先拉子项
  const toggle = useCallback(
    (path: string) => {
      setExpanded((cur) => {
        const next = new Set(cur);
        if (next.has(path)) next.delete(path);
        else next.add(path);
        return next;
      });
      if (!childrenRef.current[path]) void loadDir(path);
    },
    [loadDir],
  );

 // 逐级展开定位(跳转/进入深层目录):chain = 从外层到目标自身
  const reveal = useCallback(
    async (chain: string[]) => {
      for (const seg of chain) {
        setExpanded((cur) => new Set(cur).add(seg));
        if (!childrenRef.current[seg]) await loadDir(seg);
      }
    },
    [loadDir],
  );

 // 全量清缓存重载(手动同步等)
  const reset = useCallback(
    (reloadRoot: string) => {
      setChildren({});
      setExpanded(new Set(opts.initialExpanded ?? []));
      void loadDir(reloadRoot);
    },
    [loadDir, opts.initialExpanded],
  );

  const nodes = flattenTree(children, expanded, loadingDir);
  return { children, expanded, loadingDir, nodes, loadDir, toggle, reveal, reset, setExpanded };
}

/** 展开态拍平成可见行;baseDepth 供合成根节点(如「此电脑」)占第 0 层。 */
export function flattenTree(
  children: Record<string, LazyTreeEntry[]>,
  expanded: ReadonlySet<string>,
  loadingDir: string | null,
  rootKey = "",
  baseDepth = 0,
): FileTreeNode[] {
  const nodes: FileTreeNode[] = [];
  const walk = (dirKey: string, depth: number) => {
    for (const e of children[dirKey] ?? []) {
      if (e.kind === "folder") {
        const isOpen = expanded.has(e.path);
        nodes.push({
          ...e,
          depth,
          expanded: isOpen,
          loading: loadingDir === e.path,
        });
        if (isOpen) walk(e.path, depth + 1);
      } else {
        nodes.push({ ...e, depth });
      }
    }
  };
  walk(rootKey, baseDepth);
  return nodes;
}

export function FileTree({
  nodes,
  onNodeClick,
  onNodeToggle,
  onNodeContextMenu,
  renderBadge,
  emptyText,
  className,
  ...props
}: Omit<ComponentProps<"div">, "children" | "nodes"> & {
  nodes: readonly FileTreeNode[];
  /** 行点击(带原始事件,消费方读 ctrlKey 等做多选) */
  onNodeClick?: (node: FileTreeNode, e: React.MouseEvent) => void;
  /** chevron 点击 = 仅展开/收起,不触发行点击语义(不用 onToggle 名:撞原生 div 事件) */
  onNodeToggle?: (node: FileTreeNode) => void;
  onNodeContextMenu?: (node: FileTreeNode, pos: { x: number; y: number }) => void;
  /** 行尾徽标(如选择器「已登记」) */
  renderBadge?: (node: FileTreeNode) => ReactNode;
  emptyText?: string;
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
            aria-selected={node.selected || undefined}
            onClick={(e) => onNodeClick?.(node, e)}
            onContextMenu={(e) => {
              if (!onNodeContextMenu) return;
              e.preventDefault();
              onNodeContextMenu(node, { x: e.clientX, y: e.clientY });
            }}
            className={cn(
              "flex items-center gap-2 rounded-lg px-1 py-1 text-left text-[13px] transition-colors duration-150",
              node.selected
                ? "bg-primary/10"
                : "hover:bg-foreground/[0.05]",
            )}
            style={{ paddingInlineStart: `${0.25 + node.depth * 0.85}rem` }}
          >
            {node.kind === "folder" ? (
              <>
                <span
                  className="-m-0.5 flex shrink-0 p-0.5"
                  onClick={(e) => {
                    if (!onNodeToggle) return;
                    e.stopPropagation();
                    onNodeToggle(node);
                  }}
                >
                  {node.expanded ? (
                    <ChevronDownIcon className="text-foreground/25 size-3" />
                  ) : (
                    <ChevronRightIcon className="text-foreground/25 size-3" />
                  )}
                </span>
                {node.drive ? (
                  <HardDriveIcon className="text-foreground/35 size-3.5 shrink-0" />
                ) : (
                  <FolderIcon className="text-foreground/35 size-3.5 shrink-0" />
                )}
                <span
                  className={cn(
                    "min-w-0 flex-1 truncate",
                    node.selected ? "text-foreground/90" : "text-foreground/70",
                  )}
                >
                  {node.name}
                </span>
                {renderBadge?.(node)}
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
                <span
                  className={cn(
                    "min-w-0 flex-1 truncate",
                    node.selected ? "text-foreground/90" : "text-foreground/85",
                  )}
                >
                  {node.name}
                </span>
                {renderBadge?.(node)}
              </>
            )}
          </button>
        ))}
        {nodes.length === 0 ? (
          <div className={cn(mono, "text-foreground/30 px-2 py-3")}>
            {emptyText ?? "空目录"}
          </div>
        ) : null}
      </div>
    </div>
  );
}
