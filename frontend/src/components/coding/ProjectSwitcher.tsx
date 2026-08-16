/**
 * 编程壳项目切换器（2026-08-15）：CodingApp 头部左栏——当前项目下拉。
 *
 * 项目 = 前端项目集合（localStorage 持久化）+ 后端 workspace root 参数化
 * （root 缺省 = 配置工作目录兜底）。切换项目：文件树回新项目根（清空预览，
 * 编辑器内容属于旧项目不跨项目保留）；GitBar/分支图经 currentProjectId
 * 订阅自动刷新；新开终端以项目根为 cwd。
 *
 * 新建项目 = 名称 + 根目录绝对路径（本地桌面应用无目录选择器，直接输路径）；
 * 提交前用 /api/workspace/list 探测路径可访问（不可用 → toast 提示）。
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Folder, FolderKanban, Loader2, Plus, X } from "lucide-react";
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
import { api } from "@/api/client";
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

/** 新建项目弹窗：名称 + 根目录路径（绝对路径；提交前探测可访问性） */
function ProjectDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  const addProject = useAppStore((s) => s.addProject);
  const [name, setName] = useState("");
  const [root, setRoot] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    const path = root.trim();
    if (!path || busy) return;
    setBusy(true);
    try {
      // 探测路径可访问（list 目录即验证；不可访问 → 后端报错）
      await api.listWorkspace("", path);
      addProject(name, path);
      setName("");
      setRoot("");
      onClose();
    } catch (err) {
      toast.error(t("coding.project.pathInvalid", { msg: String(err) }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(v: boolean) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("coding.project.new")}</DialogTitle>
          <DialogDescription>{t("coding.project.newDesc")}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-3">
          <div className="grid gap-1.5">
            <Label htmlFor="project-name">{t("coding.project.name")}</Label>
            <Input
              id="project-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("coding.project.namePlaceholder")}
              className="h-8 text-sm"
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="project-root">{t("coding.project.root")}</Label>
            <Input
              id="project-root"
              value={root}
              onChange={(e) => setRoot(e.target.value)}
              placeholder={t("coding.project.rootPlaceholder")}
              className="h-8 font-mono text-xs"
              onKeyDown={(e) => {
                if (e.key === "Enter") void submit();
              }}
            />
          </div>
        </div>
        <DialogFooter>
          <Button size="sm" disabled={!root.trim() || busy} onClick={() => void submit()}>
            {busy && <Loader2 size={12} className="animate-spin" />}
            {t("coding.project.create")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
