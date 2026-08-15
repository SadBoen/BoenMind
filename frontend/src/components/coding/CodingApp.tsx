/**
 * 编程应用独立壳（M2，用户拍板"编程为软件实现第一优先"）：
 * ┌───────────────────────────────────────┐
 * │ GitBar：分支 + 最近提交节点 + 变更摘要    │
 * ├────────┬─────────────────┬────────────┤
 * │ 文件树   │ 编辑器            │ 右栏 Tab：   │
 * │        │                 │ 任务 | 对话 | 终端 │
 * └────────┴─────────────────┴────────────┘
 * 后端零新增编排概念：文件走 /api/workspace（读/写），清单走事件日志
 * todo/write 投影（REST + 事件流双通道），git 走 /api/workspace/git-info。
 * 分支图 = 起步形态（提交节点时间线）；完整 DAG 图留 M2 深化轮。
 *
 * 右栏对话 Tab = 宿主共享 ChatPane（panel 形态，架构 §四·B 补充）：对话是
 * 宿主能力非应用能力，编程壳只需嵌入并绑定 coding 场景会话（一软件一会话，
 * 无则懒创建）。终端 Tab = 宿主共享 TerminalPane（公共功能页组件化第一批，
 * 上游吸收 xterm.js + portable-pty）。桌面壳多窗口共存时与聊天窗口共享
 * 聚焦会话（多实例拍板项）。
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { GitBranch, MessageSquare, RefreshCw, ListTodo, SquareTerminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { api, type GitInfo } from "@/api/client";
import { useAppStore } from "@/stores/app-store";
import { FilePanel } from "@/components/files/FilePanel";
import { Editor } from "./Editor";
import { TodoPanel } from "./TodoPanel";
import { ChatPane } from "@/components/chat/ChatPane";
import { TerminalPane } from "@/components/terminal/TerminalPane";

/** 右栏 Tab：任务（活任务清单投影）| 对话（宿主 ChatPane）| 终端（宿主 TerminalPane） */
type RightTab = "tasks" | "chat" | "terminal";

export function CodingApp() {
  const { t } = useTranslation();
  const [git, setGit] = useState<GitInfo | null>(null);
  const [rightTab, setRightTab] = useState<RightTab>("tasks");
  const ensureAppSession = useAppStore((s) => s.ensureAppSession);

  const loadGit = useCallback(() => {
    api
      .gitInfo()
      .then(setGit)
      .catch(() => setGit(null));
  }, []);

  useEffect(() => {
    loadGit();
  }, [loadGit]);

  // 对话 Tab = 真实会话入口：切过去时确保 coding 场景会话存在（无则懒创建）
  const openChatTab = () => {
    setRightTab("chat");
    void ensureAppSession("coding");
  };

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

      {/* 三栏：文件树 | 编辑器 | 右栏（任务/对话 Tab） */}
      <div className="flex min-h-0 flex-1">
        <div className={cn("w-56 shrink-0 border-r")}>
          <FilePanel />
        </div>
        <div className="min-w-0 flex-1 border-r">
          <Editor />
        </div>
        <div
          className={cn(
            "flex shrink-0 flex-col",
            rightTab === "chat" ? "w-[26rem]" : rightTab === "terminal" ? "w-[30rem]" : "w-64",
          )}
        >
          {/* Tab 头：任务 | 对话 | 终端（对话/终端 = 宿主共享组件） */}
          <div className="flex h-9 shrink-0 items-center gap-1 border-b px-2">
            <TabButton active={rightTab === "tasks"} onClick={() => setRightTab("tasks")} icon={<ListTodo size={13} />}>
              {t("coding.tabs.tasks")}
            </TabButton>
            <TabButton active={rightTab === "chat"} onClick={openChatTab} icon={<MessageSquare size={13} />}>
              {t("coding.tabs.chat")}
            </TabButton>
            <TabButton active={rightTab === "terminal"} onClick={() => setRightTab("terminal")} icon={<SquareTerminal size={13} />}>
              {t("coding.tabs.terminal")}
            </TabButton>
          </div>
          {rightTab === "tasks" ? (
            <TodoPanel />
          ) : rightTab === "chat" ? (
            <ChatPane variant="panel" />
          ) : (
            <TerminalPane />
          )}
        </div>
      </div>
    </div>
  );
}

function TabButton({
  active,
  onClick,
  icon,
  children,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex h-7 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium transition-colors",
        active
          ? "bg-primary/10 text-primary"
          : "text-muted-foreground hover:bg-accent hover:text-foreground",
      )}
    >
      {icon}
      {children}
    </button>
  );
}
