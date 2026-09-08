import React from "react";
import {
  FileCode,
  FileJson,
  FileText,
  FileImage,
  Terminal,
  File,
  Settings,
  Database,
  FileSpreadsheet,
} from "lucide-react";

export type FileTypeConfig = {
  icon: React.ComponentType<{ className?: string; size?: number }>;
  colorClass: string;
  badgeBg: string;
};

// 扩展名彩色配置字典（对照 ZCode 与 Material Icons 规范）
export function getFileConfig(filePath: string): FileTypeConfig {
  const lower = filePath.toLowerCase().trim();
  const fileName = lower.split(/[/\\]/).pop() || "";
  const ext = fileName.includes(".") ? fileName.slice(fileName.lastIndexOf(".") + 1) : "";

  // 1. 特殊文件名匹配
  if (fileName === "dockerfile" || fileName.startsWith("docker-compose")) {
    return { icon: Terminal, colorClass: "text-sky-500", badgeBg: "bg-sky-500/10" };
  }
  if (fileName.includes("cargo") || fileName.endsWith(".rs")) {
    return { icon: FileCode, colorClass: "text-amber-600 dark:text-orange-400", badgeBg: "bg-orange-500/10" };
  }
  if (fileName.startsWith(".env") || fileName.includes("config") || fileName.includes("settings")) {
    return { icon: Settings, colorClass: "text-emerald-500", badgeBg: "bg-emerald-500/10" };
  }
  if (fileName.includes("sqlite") || fileName.endsWith(".db") || fileName.endsWith(".sql")) {
    return { icon: Database, colorClass: "text-blue-500", badgeBg: "bg-blue-500/10" };
  }

  // 2. 常见语言后缀匹配
  switch (ext) {
    case "ts":
    case "tsx":
      return { icon: FileCode, colorClass: "text-blue-500", badgeBg: "bg-blue-500/10" };
    case "js":
    case "jsx":
      return { icon: FileCode, colorClass: "text-yellow-500", badgeBg: "bg-yellow-500/10" };
    case "py":
      return { icon: FileCode, colorClass: "text-emerald-600 dark:text-emerald-400", badgeBg: "bg-emerald-500/10" };
    case "go":
      return { icon: FileCode, colorClass: "text-cyan-500", badgeBg: "bg-cyan-500/10" };
    case "json":
    case "jsonl":
    case "yaml":
    case "yml":
    case "toml":
      return { icon: FileJson, colorClass: "text-amber-500", badgeBg: "bg-amber-500/10" };
    case "md":
    case "markdown":
    case "txt":
      return { icon: FileText, colorClass: "text-indigo-400 dark:text-indigo-300", badgeBg: "bg-indigo-500/10" };
    case "csv":
    case "xlsx":
    case "xls":
      return { icon: FileSpreadsheet, colorClass: "text-emerald-500", badgeBg: "bg-emerald-500/10" };
    case "sh":
    case "bash":
    case "bat":
    case "ps1":
    case "cmd":
      return { icon: Terminal, colorClass: "text-rose-500", badgeBg: "bg-rose-500/10" };
    case "png":
    case "jpg":
    case "jpeg":
    case "svg":
    case "webp":
    case "gif":
      return { icon: FileImage, colorClass: "text-pink-500", badgeBg: "bg-pink-500/10" };
    case "css":
    case "scss":
    case "less":
      return { icon: FileCode, colorClass: "text-purple-400", badgeBg: "bg-purple-500/10" };
    default:
      return { icon: File, colorClass: "text-muted-foreground", badgeBg: "bg-muted/40" };
  }
}

/**
 * 将长路径脱敏截断为短路径（如 D:\...\runtime\src\main.rs -> src/main.rs）
 */
export function formatShortPath(fullPath: string): { display: string; full: string } {
  if (!fullPath) return { display: "", full: "" };
  const clean = fullPath.replace(/^["']|["']$/g, "").replace(/\\/g, "/");
  const segments = clean.split("/").filter(Boolean);
  if (segments.length <= 2) {
    return { display: segments.join("/"), full: clean };
  }
  // 取最后两级路径作为紧凑显示，如 crates/workspace.rs
  return {
    display: segments.slice(-2).join("/"),
    full: clean,
  };
}

export function FileBadge({
  path,
  className = "",
  showIcon = true,
}: {
  path: string;
  className?: string;
  showIcon?: boolean;
}) {
  const { display, full } = formatShortPath(path);
  const cfg = getFileConfig(full || path);
  const Icon = cfg.icon;

  return (
    <span
      className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md font-mono text-[11.5px] border border-border/40 transition-colors hover:border-border cursor-pointer select-none max-w-full truncate ${cfg.badgeBg} ${className}`}
      title={full || path}
      onClick={(e) => {
        e.stopPropagation();
        void navigator.clipboard?.writeText(full || path);
      }}
    >
      {showIcon ? <Icon className={`size-3.5 shrink-0 ${cfg.colorClass}`} /> : null}
      <span className="truncate font-medium text-foreground/90">{display || path}</span>
    </span>
  );
}
