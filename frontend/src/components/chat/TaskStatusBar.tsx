/**
 * 任务状态条：进行中任务的心跳进度（SSE taskProgress）与最近任务的终态展示。
 *
 * 断线续跑语义：后端已去掉"客户端断开即中止"——页面离开后任务继续执行，
 * 重新打开会话时由 store.lastTask（GET /api/sessions/{id}/tasks）恢复：
 * - running → 显示"任务仍在进行"（后端还在跑）；
 * - completed / failed / cancelled → 显示终态徽章。
 */
import { useTranslation } from "react-i18next";
import { CheckCircle2, Loader2, XCircle } from "lucide-react";
import { useAppStore } from "@/stores/app-store";

export function TaskStatusBar() {
  const { t } = useTranslation();
  const streaming = useAppStore((s) => s.streaming);
  const taskProgress = useAppStore((s) => s.taskProgress);
  const lastTask = useAppStore((s) => s.lastTask);
  const activeSessionId = useAppStore((s) => s.activeSessionId);
  if (!activeSessionId) return null;

  // 活跃心跳：流式期间展示最近活动摘要
  if (streaming && taskProgress) {
    return (
      <div className="flex shrink-0 items-center gap-2 border-b bg-muted/30 px-3 py-1 text-xs text-muted-foreground">
        <Loader2 size={12} className="animate-spin text-primary" />
        <span className="truncate">{taskProgress}</span>
      </div>
    );
  }

  // 断线续跑：最近任务仍 running（页面离开期间后端继续执行）
  if (!streaming && lastTask?.status === "running") {
    return (
      <div className="flex shrink-0 items-center gap-2 border-b bg-muted/30 px-3 py-1 text-xs text-muted-foreground">
        <Loader2 size={12} className="animate-spin text-primary" />
        <span className="truncate">{lastTask.progress || t("chat.taskRunning")}</span>
      </div>
    );
  }

  // 最近任务终态（含失败原因）
  if (!streaming && lastTask && lastTask.status !== "running") {
    const done = lastTask.status === "completed";
    return (
      <div className="flex shrink-0 items-center gap-2 border-b px-3 py-1 text-xs text-muted-foreground">
        {done ? (
          <CheckCircle2 size={12} className="shrink-0 text-green-600" />
        ) : (
          <XCircle
            size={12}
            className={`shrink-0 ${lastTask.status === "failed" ? "text-destructive" : ""}`}
          />
        )}
        <span className="truncate">
          {t(`chat.taskStatus.${lastTask.status}`)}
          {lastTask.error ? `：${lastTask.error}` : ""}
        </span>
      </div>
    );
  }
  return null;
}
