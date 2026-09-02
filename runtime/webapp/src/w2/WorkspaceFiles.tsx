// W2 工作区面板:文件 tab = 目录树(懒加载展开)+ 文件预览。
// 预览视图盖住目录树,左上角返回图标回树(W2 规格 §2 验收关键形态)。
// 后端 = /admin/fs/list + /admin/fs/file(只读,X-01 路径防护)。
// 目录树原型 = 注册表 elements-file-tree(选装适配见 file-tree.tsx);
// 预览视图为自有组件。
import { useCallback, useEffect, useState } from "react";
import {
  ArrowLeftIcon,
  CopyIcon,
  DownloadIcon,
  FileTextIcon,
  FolderDownIcon,
  PencilIcon,
  Loader2Icon,
} from "lucide-react";
import { api, type FsEntry } from "./api";
import { FileTree, type FileTreeNode } from "@/components/file-tree";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

type Preview = {
  path: string;
  name: string;
  size: number;
  content: string;
} | null;

type CtxMenu = { node: FileTreeNode; x: number; y: number } | null;

export function WorkspaceFiles() {
  const [dirs, setDirs] = useState<Record<string, FsEntry[]>>({});
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [loadingDir, setLoadingDir] = useState<string | null>(null);
  const [preview, setPreview] = useState<Preview>(null);
  const [loadingFile, setLoadingFile] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [root, setRoot] = useState("");
  // W7 反馈:目录树右键菜单(重命名/复制路径/下载/打包下载)
  const [ctx, setCtx] = useState<CtxMenu>(null);
  const [renaming, setRenaming] = useState<FileTreeNode | null>(null);
  const [renameName, setRenameName] = useState("");
  const [notice, setNotice] = useState<string | null>(null);

  const flash = (msg: string) => {
    setNotice(msg);
    setTimeout(() => setNotice(null), 2500);
  };

  const loadDir = useCallback(async (path: string) => {
    setLoadingDir(path);
    setError(null);
    try {
      const r = await api.fs.list(path);
      setDirs((cur) => ({ ...cur, [path]: r.entries }));
      if (path === "") setRoot(r.root);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setLoadingDir(null);
    }
  }, []);

  useEffect(() => {
    void loadDir("");
  }, [loadDir]);

  // 顶栏「同步」按钮:清空已加载目录并重载根
  useEffect(() => {
    const refresh = () => {
      setDirs({});
      setExpanded(new Set());
      setPreview(null);
      void loadDir("");
    };
    window.addEventListener("bm-ws-refresh", refresh);
    return () => window.removeEventListener("bm-ws-refresh", refresh);
  }, [loadDir]);

  const toggle = (path: string) => {
    setExpanded((cur) => {
      const next = new Set(cur);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
    if (!dirs[path] && !expanded.has(path)) void loadDir(path);
  };

  const openFile = async (path: string) => {
    setLoadingFile(true);
    setError(null);
    try {
      const r = await api.fs.file(path);
      setPreview(r);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setLoadingFile(false);
    }
  };

  const onNodeClick = (node: FileTreeNode) => {
    if (node.kind === "folder") void toggle(node.path);
    else void openFile(node.path);
  };

  // ---- W7 右键菜单动作 ----------------------------------------------------
  const absPath = (rel: string) => (root ? `${root}/${rel}` : rel);

  const copyText = async (text: string, label: string) => {
    try {
      await navigator.clipboard.writeText(text);
      flash(`已复制${label}`);
    } catch {
      // 剪贴板 API 不可用(权限/环境)时退化为选中文本提示
      setError(`复制失败:浏览器不允许访问剪贴板,请手动复制:${text}`);
    }
  };

  const triggerDownload = (rel: string) => {
    const a = document.createElement("a");
    a.href = api.fs.downloadUrl(rel);
    a.download = "";
    document.body.appendChild(a);
    a.click();
    a.remove();
    flash("已开始下载");
  };

  const doRename = async () => {
    if (!renaming || !renameName.trim()) return;
    try {
      await api.fs.rename(renaming.path, renameName.trim());
      setRenaming(null);
      setCtx(null);
      setDirs({});
      setExpanded(new Set());
      void loadDir("");
      flash("已重命名");
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  // 展开态 → 拍平节点列表(懒加载:未加载的目录只显示自身一行)
  const nodes: FileTreeNode[] = [];
  const walk = (dirPath: string, depth: number) => {
    for (const e of dirs[dirPath] ?? []) {
      const childPath = dirPath ? `${dirPath}/${e.name}` : e.name;
      if (e.kind === "dir") {
        const isOpen = expanded.has(childPath);
        nodes.push({
          path: childPath,
          name: e.name,
          depth,
          kind: "folder",
          expanded: isOpen,
          loading: loadingDir === childPath,
        });
        if (isOpen) walk(childPath, depth + 1);
      } else {
        nodes.push({ path: childPath, name: e.name, depth, kind: "file" });
      }
    }
  };
  walk("", 0);

  return (
    <div className="relative min-h-0 flex-1" data-slot="workspace-files">
      <div
        className={cn(
          "flex min-h-0 flex-col gap-2",
          preview ? "invisible" : "visible",
        )}
      >
        <FileTree
          nodes={nodes}
          onNodeClick={onNodeClick}
          onNodeContextMenu={(node, pos) => setCtx({ node, ...pos })}
          className="min-h-0 flex-1 overflow-y-auto"
        />
        {loadingFile ? (
          <div className="text-muted-foreground flex items-center gap-1.5 px-1 text-[12px]">
            <Loader2Icon className="size-3 animate-spin" />
            读取文件…
          </div>
        ) : null}
      </div>

      {error ? (
        <div className="absolute inset-x-0 bottom-0 rounded-lg border border-red-300 bg-red-50 px-3 py-2 text-[12px] text-red-700">
          {error}
        </div>
      ) : null}
      {notice ? (
        <div className="absolute inset-x-0 bottom-0 rounded-lg border border-emerald-300 bg-emerald-50 px-3 py-2 text-[12px] text-emerald-700">
          {notice}
        </div>
      ) : null}

      {/* W7 右键菜单(自绘,不用原生 contextmenu;点空白/Esc 关闭) */}
      {ctx ? (
        <div className="fixed inset-0 z-50" onClick={() => setCtx(null)} onContextMenu={(e) => { e.preventDefault(); setCtx(null); }}>
          <div
            className="bg-popover text-popover-foreground fixed min-w-44 rounded-lg border p-1 shadow-md"
            style={{
              left: Math.min(ctx.x, window.innerWidth - 190),
              top: Math.min(ctx.y, window.innerHeight - 210),
            }}
            data-slot="fs-context-menu"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="text-muted-foreground truncate px-3 py-1 font-mono text-[10.5px]">
              {ctx.node.name}
            </div>
            <button
              className="hover:bg-muted flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-[12.5px]"
              onClick={() => {
                setRenameName(ctx.node.name);
                setRenaming(ctx.node);
                setCtx(null);
              }}
            >
              <PencilIcon className="size-3.5" /> 重命名
            </button>
            <button
              className="hover:bg-muted flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-[12.5px]"
              onClick={() => {
                void copyText(absPath(ctx.node.path), "绝对路径");
                setCtx(null);
              }}
            >
              <CopyIcon className="size-3.5" /> 复制绝对路径
            </button>
            <button
              className="hover:bg-muted flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-[12.5px]"
              onClick={() => {
                void copyText(ctx.node.path, "相对路径");
                setCtx(null);
              }}
            >
              <CopyIcon className="size-3.5" /> 复制相对路径
            </button>
            {ctx.node.kind === "folder" ? (
              <button
                className="hover:bg-muted flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-[12.5px]"
                onClick={() => {
                  triggerDownload(ctx.node.path);
                  setCtx(null);
                }}
              >
                <FolderDownIcon className="size-3.5" /> 打包下载(zip)
              </button>
            ) : (
              <button
                className="hover:bg-muted flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-[12.5px]"
                onClick={() => {
                  triggerDownload(ctx.node.path);
                  setCtx(null);
                }}
              >
                <DownloadIcon className="size-3.5" /> 下载
              </button>
            )}
          </div>
        </div>
      ) : null}

      {/* W7 重命名对话框 */}
      <Dialog open={renaming !== null} onOpenChange={(v) => !v && setRenaming(null)}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>重命名</DialogTitle>
          </DialogHeader>
          <Input
            autoFocus
            value={renameName}
            onChange={(e) => setRenameName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void doRename()}
            data-slot="rename-input"
          />
          <DialogFooter>
            <Button variant="outline" size="sm" onClick={() => setRenaming(null)}>
              取消
            </Button>
            <Button size="sm" disabled={!renameName.trim()} onClick={() => void doRename()}>
              确认
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 预览:盖住目录树;左上角返回图标回树(W2 验收形态) */}
      {preview ? (
        <div
          className="bg-background absolute inset-0 z-10 flex flex-col rounded-xl border"
          data-slot="file-preview"
          data-path={preview.path}
        >
          <div className="flex items-center gap-2 border-b px-2 py-2">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setPreview(null)}
              title="返回目录树"
              data-slot="preview-back"
            >
              <ArrowLeftIcon />
            </Button>
            <span className="truncate font-mono text-[12.5px]">{preview.path}</span>
            <span className="text-muted-foreground ml-auto shrink-0 font-mono text-[11px]">
              {preview.size} B
            </span>
          </div>
          <pre className="min-h-0 flex-1 overflow-auto p-3 font-mono text-[12px] leading-relaxed whitespace-pre-wrap">
            {preview.content}
          </pre>
        </div>
      ) : null}
    </div>
  );
}
