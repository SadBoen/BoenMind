// W2 工作区面板:文件 tab = 目录树(懒加载展开)+ 文件预览。
// 预览视图盖住目录树,左上角返回图标回树(W2 规格 §2 验收关键形态)。
// 后端 = /admin/fs/list + /admin/fs/file(只读,X-01 路径防护)。
// 目录树 = 共用 FileTree/useLazyTree();本次增:
// ctrl/cmd 多选 + 右键删除(批量,确认弹窗)+ 右键/空白处新建文件夹。
import { useCallback, useEffect, useState } from "react";
import {
  ArrowLeftIcon,
  CopyIcon,
  DownloadIcon,
  FolderDownIcon,
  FolderPlusIcon,
  PencilIcon,
  Trash2Icon,
  Loader2Icon,
} from "lucide-react";
import { api } from "./api";
import { useTimedNotice } from "@/lib/use-timed-notice";
import {
  FileTree,
  useLazyTree,
  type FileTreeNode,
  type LazyTreeEntry,
} from "@/components/file-tree";
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
import { BM_EVENTS } from "../lib/bus";

type Preview = {
  path: string;
  name: string;
  size: number;
  content: string;
} | null;

type CtxMenu = { node: FileTreeNode; x: number; y: number } | null;

// 相对路径的父目录 key("a/b/c" → "a/b";顶层 → "")
function parentKey(p: string): string {
  const idx = p.lastIndexOf("/");
  return idx >= 0 ? p.slice(0, idx) : "";
}

export function WorkspaceFiles() {
  const [preview, setPreview] = useState<Preview>(null);
  const [loadingFile, setLoadingFile] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [root, setRoot] = useState("");
 // W7 反馈:目录树右键菜单(重命名/复制路径/下载/打包下载)
  const [ctx, setCtx] = useState<CtxMenu>(null);
 // 空白处右键(仅新建文件夹)
  const [areaCtx, setAreaCtx] = useState<{ x: number; y: number } | null>(null);
  const [renaming, setRenaming] = useState<FileTreeNode | null>(null);
  const [renameName, setRenameName] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirmDelete, setConfirmDelete] = useState<string[] | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [mkdirParent, setMkdirParent] = useState<string | null>(null);
  const [mkdirName, setMkdirName] = useState("");
  const { notice, flash } = useTimedNotice(2500);

  const load = useCallback(async (path: string): Promise<LazyTreeEntry[]> => {
    try {
      const r = await api.fs.list(path);
      if (path === "") setRoot(r.root);
      return r.entries.map((e) => ({
        path: path ? `${path}/${e.name}` : e.name,
        name: e.name,
        kind: e.kind === "dir" ? ("folder" as const) : ("file" as const),
      }));
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
      return [];
    }
  }, []);

  const {
    loadingDir,
    nodes,
    loadDir,
    toggle,
    reset,
    setExpanded,
  } = useLazyTree({ load });

  useEffect(() => {
    void loadDir("");
  }, [loadDir]);

 // 顶栏「同步」按钮:全量清缓存重载
  useEffect(() => {
    const refresh = () => {
      setSelected(new Set());
      setPreview(null);
      reset("");
    };
    window.addEventListener(BM_EVENTS.wsRefresh, refresh);
    return () => window.removeEventListener(BM_EVENTS.wsRefresh, refresh);
  }, [reset]);

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

 // ctrl/cmd+点击 = 切换多选(不展开/不打开预览);普通点击清空多选走原语义
  const onNodeClick = (node: FileTreeNode, e: React.MouseEvent) => {
    if (e.ctrlKey || e.metaKey) {
      setSelected((cur) => {
        const next = new Set(cur);
        if (next.has(node.path)) next.delete(node.path);
        else next.add(node.path);
        return next;
      });
      return;
    }
    if (selected.size > 0) setSelected(new Set());
    if (node.kind === "folder") toggle(node.path);
    else void openFile(node.path);
  };

 // ---- W7 右键菜单动作 ----------------------------------------------------
 // P1-33():统一正斜杠——root 来自后端 display()
 // (Windows 反斜杠形),与 rel 的正斜杠混拼出混合分隔符路径;归一为 /,
 // Windows API 与剪贴板均接受
  const absPath = (rel: string) => {
    if (!root) return rel;
    const norm = (s: string) => s.replace(/\\/g, "/");
    return `${norm(root).replace(/\/+$/, "")}/${norm(rel)}`;
  };

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
      const parent = parentKey(renaming.path);
      await api.fs.rename(renaming.path, renameName.trim());
      setRenaming(null);
      setCtx(null);
      setSelected(new Set());
      void loadDir(parent);
      flash("已重命名");
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

 // ---- )---------------------------
  const askDelete = () => {
    if (!ctx) return;
    const multi = selected.has(ctx.node.path) && selected.size > 1;
    setConfirmDelete(multi ? [...selected] : [ctx.node.path]);
    setCtx(null);
  };

  const doDelete = async () => {
    if (!confirmDelete || deleteBusy) return;
    setDeleteBusy(true);
    try {
      const r = await api.fs.delete(confirmDelete);
      const failed = r.results.filter((x) => !x.ok);
      if (preview && confirmDelete.includes(preview.path)) setPreview(null);
      if (failed.length) {
        setError(`删除失败 ${failed.length} 项:${failed[0].error ?? "未知错误"}`);
      } else {
        flash(`已删除 ${r.deleted} 项`);
      }
      const parents = [...new Set(confirmDelete.map(parentKey))];
      setConfirmDelete(null);
      setSelected(new Set());
      for (const p of parents) void loadDir(p);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setDeleteBusy(false);
    }
  };

 // ---- )------------------
  const doMkdir = async () => {
    if (mkdirParent === null || !mkdirName.trim()) return;
    try {
      await api.fs.mkdir(absPath(mkdirParent), mkdirName.trim());
      const parent = mkdirParent;
      setMkdirParent(null);
      setMkdirName("");
      setExpanded((cur) => new Set(cur).add(parent));
      void loadDir(parent);
      flash("已创建文件夹");
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

 // 右键菜单:右键未选中节点则重置为该节点;已在其选中集则保持多选
  const openCtx = (node: FileTreeNode, pos: { x: number; y: number }) => {
    if (!selected.has(node.path)) setSelected(new Set([node.path]));
    setCtx({ node, ...pos });
  };

  const ctxMulti = ctx !== null && selected.has(ctx.node.path) && selected.size > 1;
  const treeNodes = nodes.map((n) => ({ ...n, selected: selected.has(n.path) }));

  return (
    <div className="relative min-h-0 flex-1" data-slot="workspace-files">
      <div
        className={cn(
          "flex min-h-0 flex-col gap-2",
          preview ? "invisible" : "visible",
        )}
      >
        <FileTree
          nodes={treeNodes}
          onNodeClick={onNodeClick}
          onNodeToggle={(node) => toggle(node.path)}
          onNodeContextMenu={openCtx}
          className="min-h-0 flex-1 overflow-y-auto"
          onContextMenu={(e) => {
            e.preventDefault();
 // 行上的右键已由行处理器接管;此处只接空白区
            if ((e.target as HTMLElement).closest("[data-path]")) return;
            setAreaCtx({ x: e.clientX, y: e.clientY });
          }}
        />
        {loadingDir !== null ? (
          <div className="text-muted-foreground flex items-center gap-1.5 px-1 text-[12px]">
            <Loader2Icon className="size-3 animate-spin" />
            读取目录…
          </div>
        ) : null}
        {loadingFile ? (
          <div className="text-muted-foreground flex items-center gap-1.5 px-1 text-[12px]">
            <Loader2Icon className="size-3 animate-spin" />
            读取文件…
          </div>
        ) : null}
      </div>

      {error ? (
        <div className="notice-error absolute inset-x-0 bottom-0 shadow-md">
          {error}
        </div>
      ) : null}
      {notice ? (
        <div className="notice-success absolute inset-x-0 bottom-0 shadow-md">
          {notice}
        </div>
      ) : null}

      {/* W7 右键菜单(自绘;2026-09-07 增删除与新建文件夹;点空白/Esc 关闭) */}
      {ctx ? (
        <div className="fixed inset-0 z-[var(--z-dropdown,50)]" onClick={() => setCtx(null)} onContextMenu={(e) => { e.preventDefault(); setCtx(null); }}>
          <div
            className="bg-popover text-popover-foreground fixed min-w-44 rounded-lg border p-1 shadow-lg backdrop-blur-md"
            style={{
              left: Math.min(ctx.x, window.innerWidth - 190),
              top: Math.min(ctx.y, window.innerHeight - 250),
            }}
            data-slot="fs-context-menu"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="text-muted-foreground truncate px-3 py-1 font-mono text-[11px]">
              {ctxMulti ? `已选 ${selected.size} 项` : ctx.node.name}
            </div>
            {ctxMulti ? (
              <button
                className="hover:bg-muted text-destructive flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-xs transition-colors"
                onClick={askDelete}
              >
                <Trash2Icon className="size-3.5" /> 删除({selected.size})
              </button>
            ) : (
              <>
                <button
                  className="hover:bg-muted flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-xs transition-colors"
                  onClick={() => {
                    setRenameName(ctx.node.name);
                    setRenaming(ctx.node);
                    setCtx(null);
                  }}
                >
                  <PencilIcon className="size-3.5" /> 重命名
                </button>
                {ctx.node.kind === "folder" ? (
                  <button
                    className="hover:bg-muted flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-xs transition-colors"
                    onClick={() => {
                      setMkdirName("");
                      setMkdirParent(ctx.node.path);
                      setCtx(null);
                    }}
                  >
                    <FolderPlusIcon className="size-3.5" /> 新建文件夹
                  </button>
                ) : null}
                <button
                  className="hover:bg-muted flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-xs transition-colors"
                  onClick={() => {
                    void copyText(absPath(ctx.node.path), "绝对路径");
                    setCtx(null);
                  }}
                >
                  <CopyIcon className="size-3.5" /> 复制绝对路径
                </button>
                <button
                  className="hover:bg-muted flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-xs transition-colors"
                  onClick={() => {
                    void copyText(ctx.node.path, "相对路径");
                    setCtx(null);
                  }}
                >
                  <CopyIcon className="size-3.5" /> 复制相对路径
                </button>
                {ctx.node.kind === "folder" ? (
                  <button
                    className="hover:bg-muted flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-xs transition-colors"
                    onClick={() => {
                      triggerDownload(ctx.node.path);
                      setCtx(null);
                    }}
                  >
                    <FolderDownIcon className="size-3.5" /> 打包下载(zip)
                  </button>
                ) : (
                  <button
                    className="hover:bg-muted flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-xs transition-colors"
                    onClick={() => {
                      triggerDownload(ctx.node.path);
                      setCtx(null);
                    }}
                  >
                    <DownloadIcon className="size-3.5" /> 下载
                  </button>
                )}
                <button
                  className="hover:bg-muted text-destructive flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-xs transition-colors"
                  onClick={askDelete}
                >
                  <Trash2Icon className="size-3.5" /> 删除
                </button>
              </>
            )}
          </div>
        </div>
      ) : null}

      {/* 空白处右键:仅新建文件夹(建在工作区根) */}
      {areaCtx ? (
        <div className="fixed inset-0 z-[var(--z-dropdown,50)]" onClick={() => setAreaCtx(null)} onContextMenu={(e) => { e.preventDefault(); setAreaCtx(null); }}>
          <div
            className="bg-popover text-popover-foreground fixed min-w-44 rounded-lg border p-1 shadow-lg backdrop-blur-md"
            style={{
              left: Math.min(areaCtx.x, window.innerWidth - 190),
              top: Math.min(areaCtx.y, window.innerHeight - 80),
            }}
            data-slot="fs-area-context-menu"
            onClick={(e) => e.stopPropagation()}
          >
            <button
              className="hover:bg-muted flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-xs transition-colors"
              onClick={() => {
                setMkdirName("");
                setMkdirParent("");
                setAreaCtx(null);
              }}
            >
              <FolderPlusIcon className="size-3.5" /> 新建文件夹
            </button>
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

      {/* 2026-09-07 新建文件夹对话框 */}
      <Dialog open={mkdirParent !== null} onOpenChange={(v) => !v && setMkdirParent(null)}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>新建文件夹</DialogTitle>
          </DialogHeader>
          <p className="text-muted-foreground text-[12px]">
            位置:{mkdirParent === "" ? "工作区根目录" : mkdirParent}
          </p>
          <Input
            autoFocus
            value={mkdirName}
            placeholder="文件夹名称"
            onChange={(e) => setMkdirName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void doMkdir()}
            data-slot="mkdir-input"
          />
          <DialogFooter>
            <Button variant="outline" size="sm" onClick={() => setMkdirParent(null)}>
              取消
            </Button>
            <Button size="sm" disabled={!mkdirName.trim()} onClick={() => void doMkdir()}>
              创建
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 2026-09-07 删除确认弹窗(永久删除,不进回收站) */}
      <Dialog open={confirmDelete !== null} onOpenChange={(v) => !v && setConfirmDelete(null)}>
        <DialogContent className="sm:max-w-sm" data-slot="delete-confirm">
          <DialogHeader>
            <DialogTitle>确认删除</DialogTitle>
          </DialogHeader>
          <p className="text-muted-foreground text-[12.5px]">
            将永久删除 {confirmDelete?.length ?? 0} 项(文件夹整棵删除),不可恢复。
          </p>
          <DialogFooter>
            <Button variant="outline" size="sm" onClick={() => setConfirmDelete(null)}>
              取消
            </Button>
            <Button
              variant="destructive"
              size="sm"
              disabled={deleteBusy}
              data-slot="delete-confirm-go"
              onClick={() => void doDelete()}
            >
              {deleteBusy ? <Loader2Icon className="size-3.5 animate-spin" /> : "删除"}
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
