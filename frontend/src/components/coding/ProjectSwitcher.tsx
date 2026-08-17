/**
 * 编程壳项目切换器（2026-08-15）：CodingApp 头部左栏——当前项目下拉。
 *
 * 项目 = 前端项目集合（localStorage 持久化）+ 后端 workspace root 参数化
 * （root 缺省 = 配置工作目录兜底）。切换项目：文件树回新项目根（清空预览，
 * 编辑器内容属于旧项目不跨项目保留）；GitBar/分支图经 currentProjectId
 * 订阅自动刷新；新开终端以项目根为 cwd。
 *
 * 新建项目（2026-08-17 完善）：目录浏览器选择父目录（/api/workspace/browse，
 * 空路径 = 系统根盘符）→ 填名称（默认取当前目录名）→ 可选 git init →
 * 后端建目录 + 自动登记 trusted_project_roots 白名单（无需手动改设置）。
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Folder,
  FolderKanban,
  FolderOpen,
  HardDrive,
  Loader2,
  Plus,
  RefreshCw,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { api, type BrowseResult, type FileEntry } from "@/api/client";
import { useAppStore } from "@/stores/app-store";

export function ProjectSwitcher() {
  const { t } = useTranslation();
  const projects = useAppStore((s) => s.projects);
  const currentProjectId = useAppStore((s) => s.currentProjectId);
  const selectProject = useAppStore((s) => s.selectProject);
  const removeProject = useAppStore((s) => s.removeProject);
  const [open, setOpen] = useState(false);
  const [dialogOpen, setDialogOpen] = useState(false);
  const current = projects.find((p) => p.id === currentProjectId);

  return (
    <>
      <div className="relative">
        <Button
          variant="outline"
          size="sm"
          className="h-7 w-full gap-1.5 text-xs"
          title={t("coding.project.select")}
          onClick={() => setOpen((o) => !o)}
        >
          <FolderKanban size={13} className="shrink-0 text-muted-foreground" />
          <span className="min-w-0 flex-1 truncate">
            {current ? current.name : t("coding.project.select")}
          </span>
          <ChevronDown size={12} className={cn("shrink-0 transition-transform", open && "rotate-180")} />
        </Button>

        {open && (
          <>
            {/* 点击外部关闭（fixed 全屏层，位于面板下方） */}
            <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
            <div className="absolute left-0 top-full z-50 mt-1 w-full rounded-lg border bg-popover p-1.5 shadow-md">
              {projects.length === 0 && (
                <p className="px-2 py-4 text-center text-xs text-muted-foreground">
                  {t("coding.project.empty")}
                </p>
              )}
              {projects.map((p) => (
                <div
                  key={p.id}
                  role="button"
                  tabIndex={0}
                  title={p.root}
                  onClick={() => {
                    selectProject(p.id);
                    setOpen(false);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      selectProject(p.id);
                      setOpen(false);
                    }
                  }}
                  className={cn(
                    "group flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 hover:bg-accent",
                    p.id === currentProjectId && "bg-accent/60",
                  )}
                >
                  <Folder
                    size={13}
                    className={cn("shrink-0", p.id === currentProjectId ? "text-primary" : "text-muted-foreground")}
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-xs font-medium">{p.name}</span>
                    <span className="block truncate text-[10px] text-muted-foreground">{p.root}</span>
                  </span>
                  <button
                    type="button"
                    title={t("coding.project.delete")}
                    aria-label={t("coding.project.delete")}
                    onClick={(e) => {
                      e.stopPropagation();
                      removeProject(p.id);
                    }}
                    className="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 transition-opacity hover:bg-destructive/10 hover:text-destructive group-hover:opacity-100"
                  >
                    <X size={12} />
                  </button>
                </div>
              ))}
              <div className="mt-1 border-t pt-1">
                <button
                  type="button"
                  className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-accent"
                  onClick={() => {
                    setOpen(false);
                    setDialogOpen(true);
                  }}
                >
                  <Plus size={13} />
                  {t("coding.project.new")}
                </button>
              </div>
            </div>
          </>
        )}
      </div>

      <ProjectDialog open={dialogOpen} onClose={() => setDialogOpen(false)} />
    </>
  );
}

/** 新建项目弹窗：目录浏览器选父目录 + 名称 + git init。 */
function ProjectDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  const addProject = useAppStore((s) => s.addProject);
  const config = useAppStore((s) => s.config);
  const [cur, setCur] = useState("");
  const [browse, setBrowse] = useState<BrowseResult | null>(null);
  const [browseError, setBrowseError] = useState<string | null>(null);
  const [parent, setParent] = useState("");
  const [name, setName] = useState("");
  const [gitInit, setGitInit] = useState(true);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(false);
  // 目录历史（"上一级/下一级"）
  const [history, setHistory] = useState<string[]>([]);

  /** 首次打开：默认定位到配置工作目录（后端兜底根之一） */
  useEffect(() => {
    if (open) {
      setHistory([]);
      void openDir(config?.working_dir ?? "");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const openDir = useCallback(async (dir: string) => {
    setLoading(true);
    setBrowseError(null);
    try {
      const result = await api.browseWorkspace(dir);
      setBrowse(result);
      setCur(result.path);
      // 首次进入新目录时记住上一级（供返回）
      setHistory((h) => (h.length > 0 && h[h.length - 1] === result.parent ? h : [...h, result.parent]));
      setParent(result.path || result.parent);
    } catch (err) {
      setBrowseError(err instanceof Error ? err.message : String(err));
      setBrowse(null);
    } finally {
      setLoading(false);
    }
  }, []);

  const currentEntries = useMemo(() => {
    if (!browse) return [];
    return browse.entries
      .filter((e) => e.is_dir)
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [browse]);

  /** 名称默认 = 当前目录名（每次进入父目录刷新一次） */
  useEffect(() => {
    if (!cur) {
      setName("");
      return;
    }
    const parts = cur.replace(/[\\/]+$/, "").split(/[\\/]/);
    setName(parts[parts.length - 1] || "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cur]);

  /** 进入选中子目录 */
  const enterDir = (entry: FileEntry) => {
    const next = cur ? joinPath(cur, entry.name) : entry.name;
    void openDir(next);
  };

  const goUp = () => {
    if (!browse?.parent) return;
    void openDir(browse.parent);
  };

  const goBack = () => {
    if (history.length < 2) return;
    const target = history[history.length - 2];
    setHistory((h) => h.slice(0, -1));
    void openDir(target);
  };

  const submit = async () => {
    const dir = parent.trim();
    const projectName = name.trim();
    if (!dir || !projectName || busy) return;
    setBusy(true);
    try {
      const res = await api.newProject({ parent: dir, name: projectName, git_init: gitInit });
      addProject(projectName, res.root);
      toast.success(t("coding.project.created"));
      setParent("");
      setName("");
      onClose();
    } catch (err) {
      toast.error(t("coding.project.pathInvalid", { msg: String(err) }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(v: boolean) => !v && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("coding.project.new")}</DialogTitle>
          <DialogDescription>{t("coding.project.newDesc")}</DialogDescription>
        </DialogHeader>

        {/* 目录浏览器 */}
        <div className="rounded-lg border">
          <div className="flex items-center gap-1 border-b bg-muted/30 p-1">
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              title={t("coding.project.up")}
              disabled={!browse?.parent}
              onClick={() => void goUp()}
            >
              <ChevronLeft size={13} />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              title={t("coding.project.back")}
              disabled={history.length < 2}
              onClick={goBack}
            >
              <ChevronRight size={13} />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              title={t("coding.project.refresh")}
              onClick={() => void openDir(cur)}
            >
              <RefreshCw size={12} />
            </Button>
            <div className="min-w-0 flex-1 truncate px-1 font-mono text-[11px] text-muted-foreground">
              {cur ? (
                <span className="inline-flex items-center gap-1">
                  <HardDrive size={11} className="shrink-0" />
                  <span className="truncate">{cur}</span>
                </span>
              ) : (
                t("coding.project.browseRoot")
              )}
            </div>
          </div>
          <div className="max-h-44 overflow-y-auto p-1">
            {loading ? (
              <div className="flex items-center justify-center gap-2 py-6 text-xs text-muted-foreground">
                <Loader2 size={13} className="animate-spin" />
                {t("statusbar.loadingDir")}
              </div>
            ) : browseError ? (
              <p className="px-2 py-4 text-center text-xs text-destructive">{browseError}</p>
            ) : currentEntries.length === 0 ? (
              <p className="px-2 py-4 text-center text-xs text-muted-foreground">
                {t("coding.project.noSubdirs")}
              </p>
            ) : (
              currentEntries.map((entry) => (
                <button
                  key={entry.path}
                  type="button"
                  className="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs hover:bg-accent"
                  onClick={() => enterDir(entry)}
                >
                  <FolderOpen size={13} className="shrink-0 text-muted-foreground" />
                  <span className="truncate">{entry.name}</span>
                </button>
              ))
            )}
          </div>
        </div>

        <div className="grid gap-3">
          <div className="grid gap-1.5">
            <Label htmlFor="project-parent">{t("coding.project.parentDir")}</Label>
            <Input
              id="project-parent"
              value={parent}
              onChange={(e) => setParent(e.target.value)}
              placeholder="D:\\projects"
              className="h-8 font-mono text-xs"
            />
            <p className="text-[10px] text-muted-foreground">
              {t("coding.project.parentHint")}
            </p>
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="project-name">{t("coding.project.name")}</Label>
            <Input
              id="project-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("coding.project.namePlaceholder")}
              className="h-8 text-sm"
              onKeyDown={(e) => {
                if (e.key === "Enter") void submit();
              }}
            />
          </div>
          <label className="flex cursor-pointer items-center gap-2 text-xs">
            <input
              type="checkbox"
              checked={gitInit}
              onChange={(e) => setGitInit(e.target.checked)}
              className="size-3.5 accent-primary"
            />
            {t("coding.project.gitInit")}
          </label>
        </div>
        <DialogFooter>
          <Button size="sm" disabled={!parent.trim() || !name.trim() || busy} onClick={() => void submit()}>
            {busy && <Loader2 size={12} className="animate-spin" />}
            {t("coding.project.create")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** 拼接路径（兼容 / 与 \） */
function joinPath(a: string, b: string): string {
  if (!a) return b;
  const sep = a.includes("\\") ? "\\" : "/";
  return `${a.replace(/[\\/]+$/, "")}${sep}${b}`;
}
