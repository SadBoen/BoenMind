/**
 * 编程应用独立壳（M2，用户拍板"编程为软件实现第一优先"）：
 * ┌───────────────────────────────────────┐
 * │ GitBar：分支 + 最近提交节点 + 变更摘要    │
 * ├────────┬─────────────────┬────────────┤
 * │ 文件树   │ 编辑器            │ 活任务清单   │
 * └────────┴─────────────────┴────────────┘
 * 后端零新增编排概念：文件走 /api/workspace（读/写），清单走事件日志
 * todo/write 投影（REST + 事件流双通道），git 走 /api/workspace/git-info。
 * 分支图 = 起步形态（提交节点时间线）；完整 DAG 图留 M2 深化轮。
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { GitBranch, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { api, type GitInfo } from "@/api/client";
import { FilePanel } from "@/components/files/FilePanel";
import { Editor } from "./Editor";
import { TodoPanel } from "./TodoPanel";

export function CodingApp() {
  const { t } = useTranslation();
  const [git, setGit] = useState<GitInfo | null>(null);

  const loadGit = useCallback(() => {
    api
      .gitInfo()
      .then(setGit)
      .catch(() => setGit(null));
  }, []);

  useEffect(() => {
    loadGit();
  }, [loadGit]);

  return (
    <div className="flex h-full min-w-0 flex-col bg-background">
      {/* 分支图条（起步：分支 + 提交时间线 + 变更摘要） */}
      <div className="flex h-10 shrink-0 items-center gap-3 border-b px-3">
        <GitBranch size={14} className="text-muted-foreground" />
        {git?.repo ? (
          <>
            <span className="inline-flex items-center gap-1 rounded-full bg-primary/10 px-2 py-0.5 text-xs font-medium text-primary">
              {git.branch}
            </span>
            {/* 提交节点时间线（新 → 旧；横向） */}
            <div className="flex min-w-0 items-center gap-1 overflow-hidden" title={t("coding.git.commits")}>
              {(git.commits ?? []).map((c, i) => (
                <span key={c.hash} className="flex shrink-0 items-center gap-1">
                  {i > 0 && <span className="h-px w-3 bg-muted-foreground/30" />}
                  <span
                    className="flex items-center gap-1 rounded-full bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground"
                    title={`${c.hash} ${c.subject}`}
                  >
                    <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
                    <span className="max-w-28 truncate">{c.subject}</span>
                  </span>
                </span>
              ))}
              {(git.commits ?? []).length === 0 && (
                <span className="text-xs text-muted-foreground">{t("coding.git.noCommits")}</span>
              )}
            </div>
            {/* 变更摘要 */}
            {(git.status ?? []).length > 0 && (
              <span
                className="ml-auto shrink-0 rounded-full bg-amber-500/10 px-2 py-0.5 text-[10px] text-amber-600 dark:text-amber-400"
                title={(git.status ?? []).join("\n")}
              >
                {t("coding.git.changes", { count: (git.status ?? []).length })}
              </span>
            )}
          </>
        ) : (
          <span className="text-xs text-muted-foreground">{t("coding.git.noRepo")}</span>
        )}
        <Button
          variant="ghost"
          size="icon"
          className="ml-auto h-7 w-7 shrink-0"
          title={t("common.refresh")}
          onClick={loadGit}
        >
          <RefreshCw size={13} />
        </Button>
      </div>

      {/* 三栏：文件树 | 编辑器 | 活任务清单 */}
      <div className="flex min-h-0 flex-1">
        <div className={cn("w-56 shrink-0 border-r")}>
          <FilePanel />
        </div>
        <div className="min-w-0 flex-1 border-r">
          <Editor />
        </div>
        <div className="w-64 shrink-0">
          <TodoPanel />
        </div>
      </div>
    </div>
  );
}
