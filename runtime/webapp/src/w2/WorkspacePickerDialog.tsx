// 工作目录选择器(W8 增强;2026-09-07 目录树批次重构):顶=路径输入+跳转+
// 新建文件夹;中=单棵懒加载目录树(与聊天文件树共用 FileTree/useLazyTree,
// 根=「此电脑」列盘符,点行进入、点箭头展开);底=名称+保存。
// 数据源 = GET /admin/fs/browse(全盘只读、仅目录名);新建目录 = /admin/fs/mkdir。
// 布局与令牌对齐 plugins/ServerConfigDialog(容器 xl / 行 md / 控件 h-7·h-8)。
import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowRightIcon, FolderPlusIcon, Loader2Icon } from "lucide-react";
import { api, type WorkspaceEntry } from "./api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  FileTree,
  useLazyTree,
  type FileTreeNode,
  type LazyTreeEntry,
} from "@/components/file-tree";

export type WorkspaceDraft = { id: string | null; name: string; path: string };

// 逐级展开定位链(与 browse 返回的规范路径同形:盘符带尾分隔,其余不带)
function chainOf(p: string): string[] {
  if (!p) return [];
  const out: string[] = [];
  if (/^[a-zA-Z]:[\\/]/.test(p)) {
    const parts = p.split(/[\\/]+/).filter(Boolean);
    let acc = `${parts[0]}\\`;
    out.push(acc);
    for (let i = 1; i < parts.length; i++) {
      if (!acc.endsWith("\\")) acc += "\\";
      acc += parts[i];
      out.push(acc);
    }
  } else {
    let acc = "";
    for (const seg of p.split("/").filter(Boolean)) {
      acc += `/${seg}`;
      out.push(acc);
    }
  }
  return out;
}

// 末段目录名(盘符根 "D:\" → "D:",根视图 → "此电脑")
function baseNameOf(p: string): string {
  if (!p) return "此电脑";
  const trimmed = p.replace(/[\\/]+$/, "");
  const idx = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  const base = idx >= 0 ? trimmed.slice(idx + 1) : trimmed;
  return base || p;
}

function samePath(a: string, b: string): boolean {
  if (a === b) return true;
  const win = /^[a-zA-Z]:[\\/]/;
  return win.test(a) && win.test(b) && a.toLowerCase() === b.toLowerCase();
}

export function WorkspacePickerDialog({
  draft,
  registered,
  busy,
  error,
  onClose,
  onSave,
}: {
  draft: WorkspaceDraft;
  registered: WorkspaceEntry[];
  busy: boolean;
  error: string | null;
  onClose: () => void;
  onSave: (name: string, path: string) => void;
}) {
  const [cur, setCur] = useState("");
  const [pathInput, setPathInput] = useState("");
  const [jumpErr, setJumpErr] = useState<string | null>(null);
  const [hint, setHint] = useState("");
  const [name, setName] = useState(draft.name);
  // 名称自动跟随所选目录名;用户手改过即不再覆盖
  const nameTouchedRef = useRef(false);
  // 新建文件夹:open = 行内输入行
  const [mkdirOpen, setMkdirOpen] = useState(false);
  const [mkdirName, setMkdirName] = useState("");
  const [mkdirBusy, setMkdirBusy] = useState(false);
  // 各目录的截断/不可读提示(browse note)
  const notesRef = useRef<Record<string, string>>({});

  const load = useCallback(async (path: string): Promise<LazyTreeEntry[]> => {
    try {
      const r = await api.fs.browse(path);
      notesRef.current[path] = [
        r.truncated ? `仅显示前 ${r.entries.length} 项` : "",
        r.note ?? "",
      ]
        .filter(Boolean)
        .join(" · ");
      return r.entries.map((e) => ({
        path: e.path,
        name: e.name,
        kind: "folder" as const,
        drive: path === "",
      }));
    } catch (e) {
      setJumpErr(e instanceof Error ? e.message : String(e));
      return [];
    }
  }, []);

  const { nodes, expanded, loadingDir, loadDir, toggle, reveal } = useLazyTree({
    load,
    initialExpanded: [""],
  });

  // 进入目录 = 逐级展开定位 + 置当前 + 名称自动跟随(未手改时)
  const treeBoxRef = useRef<HTMLDivElement>(null);
  const navigate = async (target: string) => {
    setJumpErr(null);
    const chain = chainOf(target);
    await reveal(chain);
    const norm = chain.length ? chain[chain.length - 1] : "";
    setCur(norm);
    setPathInput(norm);
    setHint(notesRef.current[norm] ?? "");
    if (!nameTouchedRef.current) setName(baseNameOf(norm));
    // 定位到当前目录行(深层跳转不滚动看不见)
    requestAnimationFrame(() => {
      treeBoxRef.current
        ?.querySelector('[aria-selected="true"]')
        ?.scrollIntoView({ block: "nearest" });
    });
  };

  const doMkdir = async () => {
    const nm = mkdirName.trim();
    if (!nm || !cur || mkdirBusy) return;
    setMkdirBusy(true);
    setJumpErr(null);
    try {
      const r = await api.fs.mkdir(cur, nm);
      setMkdirOpen(false);
      setMkdirName("");
      await loadDir(cur);
      await navigate(r.path);
    } catch (e) {
      setJumpErr(e instanceof Error ? e.message : String(e));
    } finally {
      setMkdirBusy(false);
    }
  };

  useEffect(() => {
    // 仅挂载时初始化一次(本组件按打开次数条件挂载,依赖故意为空);
    // 编辑既有目录时直接定位到该目录
    if (draft.path) void navigate(draft.path);
    else void loadDir("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const duplicate = registered.some(
    (w) => w.id !== draft.id && samePath(w.path, cur),
  );
  const canSave = !!cur && !!name.trim() && !busy && !duplicate;

  const save = () => {
    if (canSave) onSave(name.trim(), cur);
  };

  const treeNodes: FileTreeNode[] = [
    {
      path: "",
      name: "此电脑",
      depth: 0,
      kind: "folder",
      drive: true,
      expanded: expanded.has(""),
      loading: loadingDir === "",
      selected: cur === "",
    },
    ...nodes.map((n) => ({ ...n, selected: samePath(n.path, cur) })),
  ];

  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent
        data-slot="workspace-dialog"
        className="flex h-[min(82vh,600px)] flex-col overflow-hidden sm:max-w-[720px]"
      >
        <DialogHeader className="shrink-0">
          <DialogTitle>{draft.id ? "编辑工作目录" : "添加工作目录"}</DialogTitle>
          <DialogDescription>
            点目录名进入,点箭头展开;也可在顶部粘贴绝对路径后点「跳转」。
          </DialogDescription>
        </DialogHeader>

        {/* 顶区:路径输入 + 跳转 + 新建文件夹 */}
        <div className="flex shrink-0 items-center gap-2">
          <Input
            id="ws-path"
            data-slot="ws-picker-input"
            className="h-8 flex-1 font-mono text-[12px]"
            value={pathInput}
            placeholder="本机绝对路径,如 D:\projects\demo;留空回车看全部盘符"
            onChange={(e) => setPathInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void navigate(pathInput.trim());
            }}
          />
          <Button
            variant="secondary"
            className="h-8 shrink-0 gap-1 px-3 text-[12px]"
            disabled={loadingDir !== null}
            data-slot="ws-picker-jump"
            onClick={() => void navigate(pathInput.trim())}
          >
            <ArrowRightIcon className="size-3.5" />
            跳转
          </Button>
          <Button
            variant="secondary"
            className="h-8 shrink-0 gap-1 px-3 text-[12px]"
            disabled={cur === "" || mkdirOpen}
            title={cur === "" ? "先进入某个盘符或目录" : "在当前目录下新建文件夹"}
            data-slot="ws-picker-mkdir"
            onClick={() => {
              setMkdirName("");
              setMkdirOpen(true);
            }}
          >
            <FolderPlusIcon className="size-3.5" />
            新建文件夹
          </Button>
        </div>
        {jumpErr ? (
          <div className="text-destructive shrink-0 text-[11.5px]">{jumpErr}</div>
        ) : null}

        {/* 中区:单棵目录树(顶部可挂行内新建输入行) */}
        <div className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-hidden">
          {mkdirOpen ? (
            <div className="flex shrink-0 items-center gap-1.5 rounded-lg border bg-muted/30 px-2 py-1.5">
              <FolderPlusIcon className="text-muted-foreground size-3.5 shrink-0" />
              <span className="text-muted-foreground shrink-0 text-[11.5px] whitespace-nowrap">
                在 {baseNameOf(cur)} 下新建
              </span>
              <Input
                autoFocus
                data-slot="ws-picker-mkdir-input"
                className="h-7 flex-1 text-[12px]"
                value={mkdirName}
                placeholder="新文件夹名称,回车确认"
                onChange={(e) => setMkdirName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void doMkdir();
                  if (e.key === "Escape") setMkdirOpen(false);
                }}
              />
              <Button
                size="sm"
                className="h-7 shrink-0 px-3 text-[12px]"
                disabled={!mkdirName.trim() || mkdirBusy}
                data-slot="ws-picker-mkdir-confirm"
                onClick={() => void doMkdir()}
              >
                {mkdirBusy ? <Loader2Icon className="size-3.5 animate-spin" /> : "创建"}
              </Button>
            </div>
          ) : null}
          <div
            ref={treeBoxRef}
            className="bo-scroll-hidden min-h-0 flex-1 overflow-y-auto rounded-xl border p-1.5"
          >
            <FileTree
              nodes={treeNodes}
              onNodeClick={(node) => void navigate(node.path)}
              onNodeToggle={(node) => toggle(node.path)}
              renderBadge={(node) =>
                node.path !== "" &&
                registered.some(
                  (w) => w.id !== draft.id && samePath(w.path, node.path),
                ) ? (
                  <Badge
                    variant="outline"
                    className="h-5 shrink-0 px-1.5 text-[10.5px]"
                  >
                    已登记
                  </Badge>
                ) : null
              }
            />
          </div>
          {hint ? (
            <div className="text-muted-foreground shrink-0 text-[11px]">{hint}</div>
          ) : null}
        </div>

        {/* 底区:名称 + 取消/保存 */}
        <div className="flex shrink-0 items-center gap-2 border-t pt-3">
          {error ? (
            <div className="notice-error min-w-0 flex-1 truncate" data-slot="workspace-error">
              {error}
            </div>
          ) : (
            <span className="flex-1" />
          )}
          {duplicate ? (
            <span className="text-amber-600 text-[11.5px] dark:text-amber-400">
              该路径已登记
            </span>
          ) : null}
          <Label htmlFor="ws-name" className="text-muted-foreground shrink-0 text-[12px]">
            名称
          </Label>
          <Input
            id="ws-name"
            className="h-8 w-44 shrink-0 text-[12px]"
            value={name}
            placeholder="如:BoenMind 项目"
            onChange={(e) => {
              nameTouchedRef.current = true;
              setName(e.target.value);
            }}
          />
          <Button variant="outline" className="h-8 shrink-0" onClick={onClose}>
            取消
          </Button>
          <Button
            className="h-8 shrink-0"
            disabled={!canSave}
            data-slot="workspace-save"
            onClick={save}
          >
            保存
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
