/**
 * 文件浏览区：工作文件夹列表 + 文件预览 + 最大化。
 * 位于最右侧，可与主面板共享区域（最大化时占自身+主区）。
 */
import { useEffect, useState } from "react";
import {
  ChevronLeft,
  File,
  FileImage,
  FileText,
  Folder,
  Maximize2,
  Minimize2,
  RefreshCw,
  Search,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn, formatBytes } from "@/lib/utils";
import type { FileEntry } from "@/api/client";
import { useAppStore } from "@/stores/app-store";
import { FilePreview } from "./FilePreview";

export function FilePanel() {
  const workspaceDir = useAppStore((s) => s.workspaceDir);
  const entries = useAppStore((s) => s.entries);
  const loadingFiles = useAppStore((s) => s.loadingFiles);
  const previewFile = useAppStore((s) => s.previewFile);
  const fileMaximized = useAppStore((s) => s.fileMaximized);
  const navigateDir = useAppStore((s) => s.navigateDir);
  const openFile = useAppStore((s) => s.openFile);
  const toggleFileMaximized = useAppStore((s) => s.toggleFileMaximized);
  const health = useAppStore((s) => s.health);

  const [query, setQuery] = useState("");

  // 首次挂载时加载工作文件夹
  useEffect(() => {
    void navigateDir("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [health?.workingDir]);

  const dirName = workspaceDir === "" ? "工作文件夹" : workspaceDir.split("/").pop();

  const filtered = entries.filter((e) => e.name.toLowerCase().includes(query.trim().toLowerCase()));

  return (
    <div className="flex h-full min-w-0 flex-col bg-background">
      {/* 头部 */}
      <div className="flex h-11 shrink-0 items-center gap-1 border-b px-2">
        {workspaceDir !== "" ? (
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            title="上一级"
            onClick={() => void navigateDir(workspaceDir.split("/").slice(0, -1).join("/"))}
          >
            <ChevronLeft size={15} />
          </Button>
        ) : null}
        <span className="min-w-0 flex-1 truncate px-1 text-sm font-medium" title={workspaceDir}>
          {dirName}
        </span>
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          title="刷新"
          onClick={() => void navigateDir(workspaceDir)}
        >
          <RefreshCw size={14} className={cn(loadingFiles && "animate-spin")} />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          title={fileMaximized ? "还原" : "最大化"}
          onClick={toggleFileMaximized}
        >
          {fileMaximized ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
        </Button>
      </div>

      {/* 文件列表 / 预览 */}
      {previewFile ? (
        <FilePreview file={previewFile} />
      ) : (
        <>
          <div className="px-2 pb-2">
            <div className="relative">
              <Search size={13} className="absolute left-2 top-1/2 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="筛选文件…"
                className="h-7 pl-7 text-xs"
              />
            </div>
          </div>
          <ScrollArea className="min-h-0 flex-1">
            <div className="px-2 pb-3">
              {filtered.length === 0 && (
                <p className="py-8 text-center text-xs text-muted-foreground">
                  {loadingFiles ? "加载中…" : "此文件夹为空"}
                </p>
              )}
              {filtered.map((entry) => (
                <FileRow key={entry.path} entry={entry} onOpen={openFile} onNavigate={(dir) => void navigateDir(dir)} />
              ))}
            </div>
          </ScrollArea>
        </>
      )}
    </div>
  );
}

function FileRow({
  entry,
  onOpen,
  onNavigate,
}: {
  entry: FileEntry;
  onOpen: (e: FileEntry) => void;
  onNavigate: (dir: string) => void;
}) {
  const Icon = entry.is_dir ? Folder : iconFor(entry.name);
  return (
    <button
      type="button"
      onClick={() => (entry.is_dir ? onNavigate(entry.path) : onOpen(entry))}
      className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-accent/50"
      title={entry.path}
    >
      <Icon size={15} className={cn("shrink-0", entry.is_dir ? "text-sky-500" : "text-muted-foreground")} />
      <span className="min-w-0 flex-1 truncate">{entry.name}</span>
      {!entry.is_dir && (
        <span className="shrink-0 text-[10px] text-muted-foreground">
          {formatBytes(entry.size)}
        </span>
      )}
    </button>
  );
}

function iconFor(name: string) {
  const ext = name.split(".").pop()?.toLowerCase();
  if (["png", "jpg", "jpeg", "gif", "webp", "svg"].includes(ext ?? "")) return FileImage;
  if (["md", "markdown", "txt", "json", "pdf"].includes(ext ?? "")) return FileText;
  return File;
}
