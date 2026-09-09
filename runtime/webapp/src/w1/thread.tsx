// W1 对话区:assistant-ui 原语组合(Thread/Message/Composer)
// 合约映射见 milestones/W1-implementation-spec.md §5
// W5:对话区页签(对话/上下文)——上下文页 = 每次模型调用请求快照透视
// W4b:对话内审批卡片(ApprovalCards)——工具调用需审批时在流内渲染
// #22 拆分:本文件为接线装配层;视图件下放 thread/ 目录——
// WindowedMessages、UserMessage、AssistantMessage、ApprovalDrawer、Composer。
import {
  ThreadPrimitive,
  useAuiState,
} from "@assistant-ui/react";
import {
  PanelLeft,
  PanelRight,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { ContextView } from "./context";
import { JobsBadge } from "./components/JobsBadge";
import { ProviderHealthBadge } from "./components/ProviderHealthBadge";
import { storage, STORAGE_KEYS } from "@/lib/storage";
import { BM_EVENTS, on } from "../lib/bus";
import { DotScrollbar } from "./DotScrollbar";
import { AgentStatusBar } from "./components/AgentStatusBar";
import { WindowedMessages } from "./thread/WindowedMessages";
import { ApprovalDrawer } from "./thread/ApprovalDrawer";
import { Composer } from "./thread/Composer";

export function Thread({
  sessionsCollapsed,
  workspaceCollapsed,
  onToggleSessions,
  onToggleWorkspace,
}: {
  sessionsCollapsed: boolean;
  workspaceCollapsed: boolean;
  onToggleSessions: () => void;
  onToggleWorkspace: () => void;
}) {
  const isEmpty = useAuiState((s) => s.thread.isEmpty);
  // W5 页签:对话 = 聊天;上下文 = 请求快照透视(dsh-context 同款布局理念)
  const [tab, setTab] = useState<"chat" | "ctx">("chat");

  const viewportRef = useRef<HTMLDivElement | null>(null);

  // 新建对话时:若在上下文透视页则自动切回对话页,并使输入框获得焦点
  useEffect(() => {
    const onNewChat = () => {
      setTab("chat");
      setTimeout(() => {
        const input = document.querySelector<HTMLTextAreaElement>(".composer-input");
        if (input) {
          input.value = "";
          input.focus();
        }
      }, 0);
    };
    window.addEventListener(BM_EVENTS.chatNew, onNewChat);
    return () => window.removeEventListener(BM_EVENTS.chatNew, onNewChat);
  }, []);
  const tabCls = (active: boolean) =>
    "rounded-full px-2.5 py-0.5 text-[12px] transition-colors " +
    (active
      ? "bg-primary text-primary-foreground"
      : "text-muted-foreground hover:bg-muted");
  // W7 反馈:左右栏抽屉开关放页签行两端(门型图标,与左侧导航同族 lucide)
  const drawerCls = (collapsed: boolean) =>
    "inline-flex h-7 w-7 items-center justify-center rounded-md transition-colors " +
    (collapsed
      ? "bg-muted text-foreground"
      : "text-muted-foreground hover:bg-muted");
  // 2026-09 审计修复:Hook 调用不得出现在 JSX/条件分支内(rules-of-hooks 违规,
  // 会话切换时可能触发「Rendered more hooks than during the previous render」)。
  // 提取到组件顶层,保证每次渲染钩子调用顺序恒定。
  const agentRunning = useAuiState((s) => s.thread.isRunning);
  // P1-32(2026-09-07 架构评审):渲染期不再直读 localStorage——改 state +
  // 事件订阅(Composer 切模型时 emit bm-active-model-changed)
  const [activeModel, setActiveModel] = useState(
    () => storage.get(STORAGE_KEYS.ACTIVE_MODEL) || "",
  );
  useEffect(
    () =>
      on(BM_EVENTS.activeModelChanged, () => {
        setActiveModel(storage.get(STORAGE_KEYS.ACTIVE_MODEL) || "");
      }),
    [],
  );
  return (
    <div className="chat">
      <div className="chat-head">
        <button
          className={drawerCls(sessionsCollapsed)}
          aria-pressed={sessionsCollapsed}
          title={
            sessionsCollapsed ? "展开 SESSION 面板" : "收起 SESSION 面板"
          }
          onClick={onToggleSessions}
          data-slot="toggle-sessions"
        >
          <PanelLeft size={16} />
        </button>
        <span className="name" style={{ flex: 1 }}>
          BoenMind 对话
        </span>
        <div className="flex items-center gap-1" role="tablist">
          <button
            role="tab"
            data-slot="tab-chat"
            data-active={tab === "chat"}
            className={tabCls(tab === "chat")}
            onClick={() => setTab("chat")}
          >
            对话
          </button>
          <button
            role="tab"
            data-slot="tab-ctx"
            data-active={tab === "ctx"}
            className={tabCls(tab === "ctx")}
            onClick={() => setTab("ctx")}
          >
            上下文
          </button>
        </div>
        <span style={{ flex: 1 }} />
        <span className="badge">self-hosted</span>
        <JobsBadge />
        <ProviderHealthBadge />
        <button
          className={drawerCls(workspaceCollapsed)}
          aria-pressed={workspaceCollapsed}
          title={
            workspaceCollapsed ? "展开 WORKSPACE 面板" : "收起 WORKSPACE 面板"
          }
          onClick={onToggleWorkspace}
          data-slot="toggle-workspace"
        >
          <PanelRight size={16} />
        </button>
      </div>
      {tab === "ctx" ? (
        <ContextView />
      ) : (
        <ThreadPrimitive.Root className="thread">
          <ThreadPrimitive.Viewport className="thread-viewport" ref={viewportRef}>
            {isEmpty ? (
              <div className="welcome">
                <div className="logo">B</div>
                <h1>个人生态的 AI Runtime</h1>
                <p>自研 Agent 已就绪——直接输入,流式回复。</p>
              </div>
            ) : (
              <>
                <WindowedMessages />
              </>
            )}
          </ThreadPrimitive.Viewport>
          <DotScrollbar viewportRef={viewportRef} />
          <div className="composer-dock">
            <div className="relative mx-auto w-full max-w-[820px]">
              <AgentStatusBar isRunning={agentRunning} activeModel={activeModel} />
              <ApprovalDrawer />
              <Composer />
            </div>
          </div>
        </ThreadPrimitive.Root>
      )}
    </div>
  );
}
