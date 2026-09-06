// 工作目录选择器(W8 增强):四区布局——顶=路径输入+跳转;中=左上级链/右当前级
// 子目录;底=名称+保存。数据源 = GET /admin/fs/browse(全盘只读、仅目录名)。
// 布局与令牌对齐 plugins/ServerConfigDialog(容器 xl / 行 md / 控件 h-7·h-8)。
import { useEffect, useRef, useState } from "react";
import {
  ArrowRightIcon,
  ChevronRightIcon,
  FolderIcon,
  HardDriveIcon,
  Loader2Icon,
} from "lucide-react";
import { api, type BrowseEntry, type WorkspaceEntry } from "./api";
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
import { cn } from "@/lib/utils";

export type WorkspaceDraft = { id: string | null; name: string; path: string };

type Ancestor = { label: string; path: string };

// 上级链(含「此电脑」根视图与当前目录自身,逐级可点)
function ancestorsOf(p: string): Ancestor[] {
  if (!p) return [{ label: "此电脑", path: "" }];
  const out: Ancestor[] = [{ label: "此电脑", path: "" }];
  if (/^[a-zA-Z]:[\\/]/.test(p)) {
    const parts = p.split(/[\\/]+/).filter(Boolean);
    let acc = "";
    parts.forEach((seg, i) => {
      acc = i === 0 ? `${seg}\\` : `${acc}${seg}\\`;
      out.push({ label: i === 0 ? acc : seg, path: acc });
    });
  } else {
    let acc = "";
    out.push({ label: "/", path: "/" });
    for (const seg of p.split("/").filter(Boolean)) {
      acc += `/${seg}`;
      out.push({ label: seg, path: acc });
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
  const [entries, setEntries] = useState<BrowseEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [jumpErr, setJumpErr] = useState<string | null>(null);
  const [hint, setHint] = useState("");
  const [name, setName] = useState(draft.name);
  // 名称自动跟随所选目录名;用户手改过即不再覆盖
  const nameTouchedRef = useRef(false);

  const load = async (target: string) => {
    setLoading(true);
    setJumpErr(null);
    try {
      const r = await api.fs.browse(target);
      setCur(r.path);
      setPathInput(r.path);
      setEntries(r.entries);
      setHint(
        [r.truncated ? `仅显示前 ${r.entries.length} 项` : "", r.note ?? ""]
          .filter(Boolean)
          .join(" · "),
      );
      return r;
    } catch (e) {
      setJumpErr(e instanceof Error ? e.message : String(e));
      return null;
    } finally {
      setLoading(false);
    }
  };

  // 进入目录 = 浏览 + 名称自动跟随(未手改时)
  const navigate = async (target: string) => {
    const r = await load(target);
    if (r && !nameTouchedRef.current) setName(baseNameOf(r.path));
  };

  useEffect(() => {
    // 仅挂载时初始化一次(本组件按打开次数条件挂载,依赖故意为空)
    void load(draft.path || "");
  }, []);

  const duplicate = registered.some(
    (w) => w.id !== draft.id && samePath(w.path, cur),
  );
  const canSave = !!cur && !!name.trim() && !busy && !duplicate;
  const ancestors = ancestorsOf(cur);

  const save = () => {
    if (canSave) onSave(name.trim(), cur);
  };

  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent
        data-slot="workspace-dialog"
        className="flex h-[min(82vh,600px)] flex-col overflow-hidden sm:max-w-[720px]"
      >
        <DialogHeader className="shrink-0">
          <DialogTitle>{draft.id ? "编辑工作目录" : "添加工作目录"}</DialogTitle>
          <DialogDescription>
            逐级点选进入目录;也可在顶部粘贴绝对路径后点「跳转」。
          </DialogDescription>
        </DialogHeader>

        {/* 顶区:路径输入 + 跳转 */}
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
            disabled={loading}
            data-slot="ws-picker-jump"
            onClick={() => void navigate(pathInput.trim())}
          >
            <ArrowRightIcon className="size-3.5" />
            跳转
          </Button>
        </div>
        {jumpErr ? (
          <div className="text-destructive shrink-0 text-[11.5px]">{jumpErr}</div>
        ) : null}

        {/* 中区:左=上级链,右=当前级子目录 */}
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,2fr)_minmax(0,3fr)] overflow-hidden rounded-xl border">
          <div className="bo-scroll-hidden flex h-full min-h-0 flex-col overflow-y-auto border-r p-2.5">
            <Label className="text-muted-foreground shrink-0 text-[10.5px] font-semibold">
              上级位置
            </Label>
            <div className="mt-1.5 space-y-1">
              {ancestors.map((a, i) => {
                const isCur = i === ancestors.length - 1;
                return (
                  <button
                    key={a.path || "__root"}
                    type="button"
                    data-slot="ws-picker-ancestor"
                    onClick={() => void navigate(a.path)}
                    title={a.path || "此电脑"}
                    className={cn(
                      "flex h-7 w-full items-center gap-1.5 rounded-md border px-2 text-left text-[12px] transition-colors",
                      isCur
                        ? "border-primary/60 bg-primary/5"
                        : "border-transparent hover:bg-muted/40",
                    )}
                  >
                    {a.path === "" ? (
                      <HardDriveIcon className="text-muted-foreground size-3.5 shrink-0" />
                    ) : (
                      <FolderIcon className="text-muted-foreground size-3.5 shrink-0" />
                    )}
                    <span className="truncate">{a.label}</span>
                  </button>
                );
              })}
            </div>
          </div>
          <div className="bo-scroll-hidden flex h-full min-h-0 flex-col overflow-y-auto p-2.5">
            <Label className="text-muted-foreground shrink-0 truncate text-[10.5px] font-semibold">
              {cur === "" ? "此电脑(点盘符进入)" : `${baseNameOf(cur)} 的子目录`}
            </Label>
            <div className="mt-1.5 space-y-1">
              {loading ? (
                <div className="text-muted-foreground flex items-center gap-2 px-2 py-3 text-[12px]">
                  <Loader2Icon className="size-3.5 animate-spin" />
                  读取中…
                </div>
              ) : entries.length === 0 ? (
                <div className="text-muted-foreground px-2 py-3 text-[12px]">
                  {hint || "无子目录"}
                </div>
              ) : (
                entries.map((e) => {
                  const reg = registered.some(
                    (w) => w.id !== draft.id && samePath(w.path, e.path),
                  );
                  return (
                    <button
                      key={e.path}
                      type="button"
                      data-slot="ws-picker-dir"
                      onClick={() => void navigate(e.path)}
                      title={e.path}
                      className="flex h-7 w-full items-center gap-1.5 rounded-md border border-transparent px-2 text-left text-[12px] transition-colors hover:bg-muted/40"
                    >
                      {cur === "" ? (
                        <HardDriveIcon className="text-muted-foreground size-3.5 shrink-0" />
                      ) : (
                        <FolderIcon className="text-muted-foreground size-3.5 shrink-0" />
                      )}
                      <span className="truncate">{e.name}</span>
                      {reg ? (
                        <Badge
                          variant="outline"
                          className="ml-auto h-5 shrink-0 px-1.5 text-[10.5px]"
                        >
                          已登记
                        </Badge>
                      ) : (
                        <ChevronRightIcon className="text-muted-foreground ml-auto size-3.5 shrink-0" />
                      )}
                    </button>
                  );
                })
              )}
            </div>
          </div>
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
