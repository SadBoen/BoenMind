// W1 对话区:assistant-ui 原语组合(Thread/Message/Composer)
// 合约映射见 milestones/W1-implementation-spec.md §5
// W5:对话区页签(对话/上下文)——上下文页 = 每次模型调用请求快照透视
// W4b:对话内审批卡片(ApprovalCards)——工具调用需审批时在流内渲染
import {
  ComposerPrimitive,
  MessagePrimitive,
  ThreadPrimitive,
  useAuiState,
  unstable_useThreadMessageIds,
} from "@assistant-ui/react";
import {
  FolderOpen,
  PanelLeft,
  PanelRight,
  Send,
  ShieldAlert,
  ShieldCheck,
  Square,
  ChevronUp,
  ChevronDown,
  Zap,
  ListOrdered,
  Code2,
  X,
  Bot,
  User,
  Pencil,
  RotateCcw,
  Copy,
  Check,
  Brain,
  Cpu,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { ContextView } from "./context";
import { useBoenmindApprovals, type ApprovalRequest } from "./runtime";
import { api, type WorkspaceEntry } from "@/w2/api";
import { JobsBadge } from "./components/JobsBadge";
import { ProviderHealthBadge } from "./components/ProviderHealthBadge";
import { storage, STORAGE_KEYS, type PermissionMode, type ThinkingLevel } from "@/lib/storage";
import { BM_EVENTS, emit, on } from "../lib/bus";
import { redirectToLogin } from "@/lib/utils";
import { DotScrollbar } from "./DotScrollbar";
import { MarkdownRenderer } from "./MarkdownRenderer";
import { parseAssistantContent } from "./parser";
import { ThinkingBlock } from "./components/ThinkingBlock";
import { ToolTreeGroup } from "./components/ToolTreeGroup";
import { TerminalBlock } from "./components/TerminalBlock";
import { AgentStatusBar } from "./components/AgentStatusBar";

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

// #27 长会话窗口化渲染:DOM 只挂最近 history.windowSize 条消息(大代码块/
// 密集表格是掉帧主源),更早已加载部分经「展开更早」即时入窗不取数;
// 窗口盖满已加载且服务端还有更早时才触发 loadOlder 取数。消息经
// Unstable_MessageById(id 锚定,窗口化/重排不脱钩)渲染,assistant-ui
// 消息上下文链与 UserMessage/AssistantMessage 组件零改动。
const MSG_WINDOW_STEP = 200;
function WindowedMessages() {
  const { history } = useBoenmindApprovals();
  const ids = unstable_useThreadMessageIds();
  const hidden = Math.max(0, ids.length - history.windowSize);
  const visible = hidden > 0 ? ids.slice(hidden) : ids;
  const topCtl =
    hidden > 0 ? (
      <div className="flex justify-center py-2">
        <button
          onClick={history.expandOlder}
          data-slot="expand-older"
          className="rounded-md border border-border/50 bg-background/70 px-3 py-1 text-[12px] text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground"
        >
          展开更早 {Math.min(hidden, MSG_WINDOW_STEP)} 条(已加载,即时显示)
        </button>
      </div>
    ) : history.capped ? (
      <div className="flex justify-center py-2 text-[12px] text-muted-foreground">
        已达本会话累计加载上限,更早历史不再载入
      </div>
    ) : history.hasMore ? (
      <div className="flex justify-center py-2">
        <button
          onClick={history.loadOlder}
          disabled={history.loading}
          data-slot="load-older"
          className="rounded-md border border-border/50 bg-background/70 px-3 py-1 text-[12px] text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground disabled:opacity-60"
        >
          {history.loading ? "加载中…" : "加载更早消息"}
        </button>
      </div>
    ) : null;
  return (
    <>
      {topCtl}
      {visible.map((id) => (
        <ThreadPrimitive.Unstable_MessageById
          key={id}
          messageId={id}
          components={{ UserMessage, AssistantMessage }}
        />
      ))}
    </>
  );
}

// 抽屉式超薄悬浮审批条:与输入框同宽、高度接近单行、支持展开代码/批准/驳回/关闭
function ApprovalDrawer() {
  const { pendingApprovals, respondApproval } = useBoenmindApprovals();
  if (pendingApprovals.length === 0) return null;
  return (
    <div className="mb-2 flex flex-col gap-2" data-slot="approval-cards">
      {pendingApprovals.map((a) => (
        <ApprovalDrawerItem
          key={a.approval_id}
          req={a}
          onRespond={respondApproval}
        />
      ))}
    </div>
  );
}

function ApprovalDrawerItem({
  req,
  onRespond,
}: {
  req: ApprovalRequest;
  onRespond: (id: string, d: "approve" | "deny") => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [dismissed, setDismissed] = useState(false);

  if (dismissed) return null;

  const argsText =
    req.args == null
      ? "(无调用参数)"
      : JSON.stringify(req.args, null, 2);

  const handleAction = async (decision: "approve" | "deny") => {
    if (busy) return;
    setBusy(true);
    try {
      await onRespond(req.approval_id, decision);
    } finally {
      setBusy(false);
    }
  };

  // 提炼简明高亮动作摘要
  const summary = (() => {
    const a = req.args as Record<string, unknown> | null;
    if (req.capability === "system.exec" && a?.command) {
      return {
        label: "执行命令",
        detail: typeof a.command === "string" ? a.command : JSON.stringify(a.command),
      };
    }
    if ((req.capability === "fs.write" || req.capability === "fs.edit") && a?.path) {
      return {
        label: req.capability === "fs.write" ? "写入文件" : "编辑文件",
        detail: String(a.path),
      };
    }
    if (req.capability === "fs.read" && a?.path) {
      return {
        label: "读取文件",
        detail: String(a.path),
      };
    }
    return {
      label: req.capability,
      detail: a ? JSON.stringify(a) : "请求执行",
    };
  })();

  // 审计修复(2026-09-08):exec 显式 cwd 在折叠态也要可见——审批时用户往往
  // 只看命令文本, cwd 是同等重要的执行边界信息(后端越界会拒绝)。
  const execCwd = (() => {
    if (req.capability !== "system.exec" || req.args == null) return null;
    const v = (req.args as Record<string, unknown>).cwd;
    return typeof v === "string" && v.trim() ? v : null;
  })();

  return (
    <div
      className="animate-in fade-in slide-in-from-bottom-2 duration-200 group relative w-full overflow-hidden rounded-xl border border-border/80 bg-card/95 shadow-md backdrop-blur-md transition-all hover:border-border"
      data-slot="approval-card"
      data-approval-id={req.approval_id}
      data-status={req.status}
    >
      {/* 单行超薄主条：高度与单行输入框/工具条相当 (约 38px) */}
      <div className="flex min-h-[38px] items-center justify-between gap-2 px-3 py-1.5">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <div className="flex size-5 shrink-0 items-center justify-center rounded-md bg-[var(--state-warn-bg)] text-[var(--state-warn-fg)]">
            <ShieldAlert className="size-3.5" />
          </div>
          <span className="shrink-0 text-[12px] font-semibold text-foreground">
            {summary.label}
          </span>
          <span
            className="truncate font-mono text-[11.5px] text-muted-foreground"
            title={summary.detail}
          >
            {summary.detail}
          </span>
          {execCwd && (
            <span
              className="flex min-w-0 max-w-[45%] shrink-0 items-center gap-1 rounded-md bg-destructive/10 px-1.5 py-0.5 font-mono text-[11px] text-destructive"
              title={`工作目录:${execCwd}(超出工作区白名单会被拒绝)`}
            >
              <span className="shrink-0 opacity-80">cwd:</span>
              <span className="truncate">{execCwd}</span>
            </span>
          )}
        </div>

        {/* 右侧紧凑操作按钮组 */}
        <div className="flex shrink-0 items-center gap-1.5">
          <Button
            size="sm"
            variant="ghost"
            className="h-7 gap-1 px-2 text-[11.5px] text-muted-foreground hover:text-foreground"
            data-slot="approval-expand"
            onClick={() => setExpanded(!expanded)}
            title={expanded ? "收起详细参数" : "展开查看详细代码/参数"}
          >
            <Code2 className="size-3.5" />
            <span>{expanded ? "收起" : "展开"}</span>
            {expanded ? (
              <ChevronUp className="size-3" />
            ) : (
              <ChevronDown className="size-3" />
            )}
          </Button>

          <Button
            size="sm"
            disabled={busy}
            className="h-7 px-2.5 text-[11.5px] font-medium shadow-xs"
            data-slot="approval-approve"
            onClick={() => void handleAction("approve")}
            title="允许本次工具调用执行"
          >
            ✓ 批准
          </Button>

          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            className="h-7 px-2 text-[11.5px] text-destructive hover:bg-destructive/10 hover:text-destructive"
            data-slot="approval-deny"
            onClick={() => void handleAction("deny")}
            title="驳回本次工具调用"
          >
            驳回
          </Button>

          <Button
            size="sm"
            variant="ghost"
            className="size-7 p-0 text-muted-foreground hover:text-foreground"
            data-slot="approval-close"
            onClick={() => setDismissed(true)}
            title="关闭悬浮条"
          >
            <X className="size-3.5" />
          </Button>
        </div>
      </div>

      {/* 展开区域：显示完整调用参数与代码详情 */}
      {expanded ? (
        <div className="border-t border-border/50 bg-muted/30 px-3 py-2">
          <div className="mb-1 flex items-center justify-between text-[10.5px] font-medium text-muted-foreground uppercase tracking-wider">
            <span>调用参数与代码详情 (Payload)</span>
            <span className="font-mono text-[10px]">{req.capability}</span>
          </div>
          <pre className="max-h-48 overflow-auto rounded-lg border border-border/60 bg-background/80 p-2.5 font-mono text-[11px] leading-relaxed text-foreground selection:bg-primary/20">
            {argsText}
          </pre>
        </div>
      ) : null}
    </div>
  );
}

function UserMessage() {
  const messageIndex = useAuiState((s) => s.message.index);
  const { editAndBranchMessage } = useBoenmindApprovals();
  const [isEditing, setIsEditing] = useState(false);
  const [editText, setEditText] = useState("");
  const [copied, setCopied] = useState(false);

  return (
    <MessagePrimitive.Root className="msg user">
      <div className="msg-header">
        <User size={13} className="text-primary" />
        <span>我</span>
      </div>
      <div className="content">
        <MessagePrimitive.Parts>
          {({ part }) => {
            if (part.type !== "text") return null;
            if (isEditing) {
              return (
                <div className="msg-inline-editor">
                  <textarea
                    className="msg-inline-textarea"
                    value={editText}
                    onChange={(e) => setEditText(e.target.value)}
                    autoFocus
                    rows={3}
                  />
                  <div className="flex items-center justify-end gap-2">
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-7 px-2 text-[12px]"
                      onClick={() => setIsEditing(false)}
                    >
                      取消
                    </Button>
                    <Button
                      size="sm"
                      className="h-7 px-3 text-[12px]"
                      onClick={async () => {
                        setIsEditing(false);
                        if (messageIndex !== undefined && editText.trim()) {
                          await editAndBranchMessage(messageIndex, editText.trim());
                        }
                      }}
                    >
                      提交并生成分支
                    </Button>
                  </div>
                </div>
              );
            }
            return (
              <div className="group/content relative flex flex-col gap-1">
                <span className="select-text whitespace-pre-wrap">{part.text}</span>
                <div className="msg-action-bar justify-end pt-1">
                  <button
                    className="msg-action-btn"
                    title="编辑本条消息并开辟分支"
                    onClick={() => {
                      setEditText(part.text);
                      setIsEditing(true);
                    }}
                  >
                    <Pencil size={12} />
                  </button>
                  <button
                    className="msg-action-btn"
                    title="复制消息"
                    onClick={async () => {
                      await navigator.clipboard.writeText(part.text);
                      setCopied(true);
                      setTimeout(() => setCopied(false), 1500);
                    }}
                  >
                    {copied ? <Check size={12} className="text-emerald-500" /> : <Copy size={12} />}
                  </button>
                </div>
              </div>
            );
          }}
        </MessagePrimitive.Parts>
      </div>
    </MessagePrimitive.Root>
  );
}

function AssistantMessage() {
  const isRunning = useAuiState((s) => s.thread.isRunning);
  const messageIndex = useAuiState((s) => s.message.index);
  const { regenerateMessage } = useBoenmindApprovals();
  const [copied, setCopied] = useState(false);

  return (
    <MessagePrimitive.Root className="msg assistant">
      <div className="msg-header">
        <Bot size={13} className="text-accent" />
        <span>BoenMind Agent</span>
        {isRunning ? <span className="badge-tag">生成中…</span> : null}
      </div>
      <div className="content">
        <MessagePrimitive.Parts>
          {({ part }) => {
            if (part.type !== "text" || !part.text) return null;
            const blocks = parseAssistantContent(part.text, isRunning);
            return (
              // P1-29(2026-09-07 架构评审):key 用索引而非文本长度——
              // 长度作 key 会在流式期间每个 delta 都重挂载整棵子树,
              // ThinkingBlock 展开态/滚动位置被反复清零
              <div className="group/content flex flex-col gap-1.5" key="content">
                {blocks.map((b, idx) => {
                  if (b.type === "thinking") {
                    return (
                      <ThinkingBlock
                        key={idx}
                        text={b.text}
                        isStreaming={b.isStreaming}
                        elapsedSeconds={b.elapsedSeconds}
                      />
                    );
                  }
                  if (b.type === "explore_group" || b.type === "changes_group" || b.type === "generic_tool_group") {
                    return <ToolTreeGroup key={idx} block={b} />;
                  }
                  if (b.type === "terminal_block") {
                    return <TerminalBlock key={idx} item={b.item} />;
                  }
                  return (
                    <MarkdownRenderer key={idx} content={b.text} />
                  );
                })}
                <div className="msg-action-bar justify-start pt-1">
                  <button
                    className="msg-action-btn"
                    title="重新生成本条回复（分支）"
                    onClick={() => {
                      if (messageIndex !== undefined) {
                        void regenerateMessage(messageIndex);
                      }
                    }}
                  >
                    <RotateCcw size={12} />
                  </button>
                  <button
                    className="msg-action-btn"
                    title="复制回复"
                    onClick={async () => {
                      await navigator.clipboard.writeText(part.text);
                      setCopied(true);
                      setTimeout(() => setCopied(false), 1500);
                    }}
                  >
                    {copied ? <Check size={12} className="text-emerald-500" /> : <Copy size={12} />}
                  </button>
                </div>
              </div>
            );
          }}
        </MessagePrimitive.Parts>
      </div>
    </MessagePrimitive.Root>
  );
}

function Composer() {
  const isRunning = useAuiState((s) => s.thread.isRunning);
  const [model, setModel] = useState("…");
  // W6:对话级模型选择——候选 = 各提供商「常用」并集(设置→模型 勾选);
  // 选择持久化 localStorage,随每条消息发给后端,中途切换下一条即生效。
  const [modelGroups, setModelGroups] = useState<Array<{ provider: string; models: string[] }>>([]);
  const [selModel, setSelModel] = useState<string>(
    () => storage.get(STORAGE_KEYS.ACTIVE_MODEL) || "",
  );
  const [roles, setRoles] = useState<Array<{ id: string; name: string }>>([]);
  const [activeRole, setActiveRole] = useState<string>(
    () => storage.get(STORAGE_KEYS.ACTIVE_ROLE) || "",
  );
  // W8:对话工作目录选择——候选 = 注册表(设置→常规 维护);选择持久化
  // localStorage 并随每条消息发给后端,中途切换下一条即生效(模型同款)。
  const [workspaces, setWorkspaces] = useState<Array<WorkspaceEntry>>([]);
  const [selWorkspace, setSelWorkspace] = useState<string>(
    () => storage.get(STORAGE_KEYS.ACTIVE_WORKSPACE) || "",
  );
  // 权限模式选择: ask(变更前确认)|plan(计划模式)|yolo(完全访问)
  const [permMode, setPermMode] = useState<PermissionMode>(
    () => (storage.get(STORAGE_KEYS.PERMISSION_MODE) as PermissionMode) || "ask",
  );
  // 思考等级选择: off(关闭)|low(轻度)|medium(中度)|high(深度)
  const [thinkingLevel, setThinkingLevel] = useState<ThinkingLevel>(
    () => (storage.get(STORAGE_KEYS.THINKING_LEVEL) as ThinkingLevel) || "medium",
  );

  const loadWorkspaces = () => {
    api.workspaces
      .list()
      .then((d) => {
        const list = d?.workspaces ?? [];
        setWorkspaces(list);
        // 已选工作区被删除 → 回落默认(不绑定)
        const cur = storage.get(STORAGE_KEYS.ACTIVE_WORKSPACE) || "";
        if (cur && !list.some((w) => w.id === cur)) {
          storage.remove(STORAGE_KEYS.ACTIVE_WORKSPACE);
          setSelWorkspace("");
        }
      })
      // P1-30(2026-09-07 架构评审):下拉加载失败不再静默,console 告警
      .catch((e) => console.warn("工作目录列表加载失败", e));
  };

  const loadModels = () => {
    api.providers.list()
      .then((d) => {
        const list: Array<{ provider: string; models: string[] }> = (d?.providers ?? [])
          .filter((p: any) => (p.modelsCommon ?? []).length > 0)
          .map((p: any) => ({ provider: p.name, models: p.modelsCommon as string[] }));
        setModelGroups(list);
        // 已选模型不在候选集(被取消勾选/删除)→ 回落服务器默认
        const all = new Set(list.flatMap((g: any) => g.models));
        const cur = storage.get(STORAGE_KEYS.ACTIVE_MODEL) || "";
        if (cur && !all.has(cur)) {
          storage.remove(STORAGE_KEYS.ACTIVE_MODEL);
          setSelModel("");
          emit(BM_EVENTS.activeModelChanged);
        }
      })
      .catch((e) => console.warn("模型候选列表加载失败", e));
  };

  const loadRoles = () => {
    api.roles.get()
      .then((d) => {
        if (d.roles && Array.isArray(d.roles)) {
          setRoles(d.roles);
          const current = storage.get(STORAGE_KEYS.ACTIVE_ROLE);
          if (!current || !d.roles.some((r: any) => r.id === current)) {
            const next = d.active_id || d.roles[0]?.id || "assistant";
            setActiveRole(next);
            storage.set(STORAGE_KEYS.ACTIVE_ROLE, next);
          } else {
            setActiveRole(current);
          }
        }
      })
      .catch((e) => console.warn("角色列表加载失败", e));
  };

  useEffect(() => {
    // P1-30:401 时正向跳登录(与 runtime.tsx 主流同口径),不再只显示 "?"
    fetch("/v1/models")
      .then((r) => {
        if (r.status === 401) {
          redirectToLogin();
        }
        return r.json();
      })
      .then((v) => setModel(v?.data?.[0]?.id ?? "?"))
      .catch(() => setModel("?"));

    loadModels();
    loadRoles();
    loadWorkspaces();
    window.addEventListener(BM_EVENTS.rolesChanged, loadRoles);
    window.addEventListener(BM_EVENTS.providersChanged, loadModels);
    window.addEventListener(BM_EVENTS.workspacesChanged, loadWorkspaces);
    return () => {
      window.removeEventListener(BM_EVENTS.rolesChanged, loadRoles);
      window.removeEventListener(BM_EVENTS.providersChanged, loadModels);
      window.removeEventListener(BM_EVENTS.workspacesChanged, loadWorkspaces);
    };
  }, []);

  const handleRoleChange = (newRoleId: string) => {
    setActiveRole(newRoleId);
    storage.set(STORAGE_KEYS.ACTIVE_ROLE, newRoleId);
    // 切换角色时清空旧会话并重开，使新角色的 system_prompt 立即绑定
    emit(BM_EVENTS.chatNew);
  };

  return (
    <ComposerPrimitive.Root className="composer">
      <ComposerPrimitive.Input
        className="composer-input"
        placeholder="Message BoenMind…"
        rows={2}
        autoFocus
      />
      <div className="composer-toolbar">
        {/* 1. 角色选择图标触发器 */}
        {roles.length > 0 ? (
          <Select value={activeRole} onValueChange={handleRoleChange}>
            <SelectTrigger
              size="icon"
              hideArrow
              className="bg-muted/60 h-7 w-7 border text-[13px] hover:bg-muted"
              title={`当前角色: ${roles.find((r) => r.id === activeRole)?.name || "默认"} (点击切换)`}
              data-slot="role-select"
            >
              <span>🎭</span>
            </SelectTrigger>
            <SelectContent className="rounded-lg" side="top" position="popper">
              {roles.map((r) => (
                <SelectItem key={r.id} value={r.id}>
                  🎭 {r.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : null}

        {/* 模型选择图标触发器 */}
        <Select
          value={selModel || "__default__"}
          onValueChange={(v) => {
            const val = v === "__default__" ? "" : v;
            setSelModel(val);
            if (val) storage.set(STORAGE_KEYS.ACTIVE_MODEL, val);
            else storage.remove(STORAGE_KEYS.ACTIVE_MODEL);
            emit(BM_EVENTS.activeModelChanged);
          }}
        >
          <SelectTrigger
            size="icon"
            hideArrow
            className="bg-muted/60 h-7 w-7 border text-muted-foreground hover:text-foreground hover:bg-muted"
            title={`当前模型: ${selModel || model} (点击切换)`}
            data-slot="model-select"
          >
            <Cpu size={14} />
          </SelectTrigger>
          <SelectContent
            className="rounded-lg max-w-xs"
            side="top"
            position="popper"
          >
            <SelectItem
              value="__default__"
              className="text-[12px] pl-2 [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
            >
              ⚙ 默认: {model}
            </SelectItem>
            {modelGroups.map((g) => (
              <SelectGroup key={g.provider}>
                <SelectLabel className="text-foreground/60 text-[12.5px] font-semibold">
                  {g.provider}
                </SelectLabel>
                {g.models.map((m) => (
                  <SelectItem
                    key={m}
                    value={m}
                    className="pl-7 text-[12px] [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
                  >
                    {m}
                  </SelectItem>
                ))}
              </SelectGroup>
            ))}
            {modelGroups.length === 0 ? (
              <SelectItem value="__none__" disabled>
                未设置常用——去 设置→模型 勾选
              </SelectItem>
            ) : null}
          </SelectContent>
        </Select>

        {/* 4. 工作目录选择图标触发器 */}
        <Select
          value={selWorkspace || "__auto__"}
          onValueChange={(v) => {
            const val = v === "__auto__" ? "" : v;
            setSelWorkspace(val);
            if (val) storage.set(STORAGE_KEYS.ACTIVE_WORKSPACE, val);
            else storage.remove(STORAGE_KEYS.ACTIVE_WORKSPACE);
          }}
        >
          <SelectTrigger
            size="icon"
            hideArrow
            className="bg-muted/60 h-7 w-7 border text-muted-foreground hover:text-foreground hover:bg-muted"
            title={`当前工作区: ${workspaces.find((w) => w.id === selWorkspace)?.name ?? "默认 (不绑定)"} (点击切换)`}
            data-slot="workspace-select"
          >
            <FolderOpen size={14} />
          </SelectTrigger>
          <SelectContent className="rounded-lg" side="top" position="popper">
            <SelectItem
              value="__auto__"
              className="text-[12px] pl-2 [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
            >
              默认 (不绑定工作目录)
            </SelectItem>
            {workspaces.map((w) => (
              <SelectItem
                key={w.id}
                value={w.id}
                className="py-1.5 pl-2 [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
              >
                <span className="flex flex-col">
                  <span className="text-[12.5px] font-medium">{w.name}</span>
                  <span className="text-muted-foreground font-mono text-[11px]">
                    {w.path}
                  </span>
                </span>
              </SelectItem>
            ))}
            {workspaces.length === 0 ? (
              <SelectItem value="__none__" disabled>
                暂无工作目录——去 设置→常规 添加
              </SelectItem>
            ) : null}
          </SelectContent>
        </Select>

        {/* 5. 思考等级选择图标触发器 (第2条遗漏补全) */}
        <Select
          value={thinkingLevel}
          onValueChange={(v) => {
            const val = v as ThinkingLevel;
            setThinkingLevel(val);
            storage.set(STORAGE_KEYS.THINKING_LEVEL, val);
          }}
        >
          <SelectTrigger
            size="icon"
            hideArrow
            className="bg-muted/60 h-7 w-7 border text-muted-foreground hover:text-foreground hover:bg-muted"
            title={`思考等级: ${{ off: "关闭", low: "轻度", medium: "中度", high: "深度" }[thinkingLevel]} (点击切换)`}
            data-slot="thinking-select"
          >
            <Brain
              size={14}
              className={
                thinkingLevel === "high"
                  ? "text-purple-500"
                  : thinkingLevel === "medium"
                  ? "text-blue-500"
                  : thinkingLevel === "low"
                  ? "text-emerald-500"
                  : "text-muted-foreground opacity-50"
              }
            />
          </SelectTrigger>
          <SelectContent className="w-48 rounded-lg p-1" side="top" position="popper">
            <SelectItem value="off" className="text-[12px]">
              <div className="flex flex-col">
                <span className="font-medium text-foreground">关闭思考</span>
                <span className="text-muted-foreground text-[10.5px]">仅快速流式直出回复</span>
              </div>
            </SelectItem>
            <SelectItem value="low" className="text-[12px]">
              <div className="flex flex-col">
                <span className="font-medium text-emerald-600">轻度思考 (Low)</span>
                <span className="text-muted-foreground text-[10.5px]">针对简单问答进行轻量推演</span>
              </div>
            </SelectItem>
            <SelectItem value="medium" className="text-[12px]">
              <div className="flex flex-col">
                <span className="font-medium text-blue-600">中度思考 (Medium)</span>
                <span className="text-muted-foreground text-[10.5px]">平衡的思考过程与输出速度</span>
              </div>
            </SelectItem>
            <SelectItem value="high" className="text-[12px]">
              <div className="flex flex-col">
                <span className="font-medium text-purple-600">深度思考 (High)</span>
                <span className="text-muted-foreground text-[10.5px]">充分展示详细推理步骤与细节</span>
              </div>
            </SelectItem>
          </SelectContent>
        </Select>

        {/* 6. 权限模式选择图标触发器 */}
        <Select
          value={permMode}
          onValueChange={(v) => {
            const val = v as PermissionMode;
            setPermMode(val);
            storage.set(STORAGE_KEYS.PERMISSION_MODE, val);
          }}
        >
          <SelectTrigger
            size="icon"
            hideArrow
            className="bg-muted/60 h-7 w-7 border text-muted-foreground hover:text-foreground hover:bg-muted"
            title={`权限模式: ${{ ask: "变更前确认", plan: "计划模式", yolo: "完全访问(免弹窗)" }[permMode]} (点击切换)`}
            data-slot="permission-select"
          >
            {permMode === "yolo" ? (
              <Zap size={14} className="text-[var(--state-warn-fg)]" />
            ) : permMode === "plan" ? (
              <ListOrdered size={14} className="text-blue-500" />
            ) : (
              <ShieldCheck size={14} className="text-[var(--state-success-fg)]" />
            )}
          </SelectTrigger>
          <SelectContent className="w-60 rounded-lg p-1" side="top" position="popper">
            <SelectItem
              value="ask"
              className="py-2 pl-2 text-[12px] [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
            >
              <div className="flex flex-col pl-4 gap-0.5">
                <div className="flex items-center gap-1.5 font-medium text-foreground">
                  <ShieldCheck className="size-3.5 text-[var(--state-success-fg)]" />
                  <span>变更前确认</span>
                </div>
                <span className="text-muted-foreground text-[11px] leading-tight">
                  修改文件与执行命令前先问我
                </span>
              </div>
            </SelectItem>

            <SelectItem
              value="plan"
              className="py-2 pl-2 text-[12px] [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
            >
              <div className="flex flex-col pl-4 gap-0.5">
                <div className="flex items-center gap-1.5 font-medium text-foreground">
                  <ListOrdered className="size-3.5 text-blue-500" />
                  <span>计划模式</span>
                </div>
                <span className="text-muted-foreground text-[11px] leading-tight">
                  编辑与改动前先给出执行计划
                </span>
              </div>
            </SelectItem>

            <SelectItem
              value="yolo"
              className="py-2 pl-2 text-[12px] [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
            >
              <div className="flex flex-col pl-4 gap-0.5">
                <div className="flex items-center gap-1.5 font-medium text-foreground">
                  <Zap className="size-3.5 text-[var(--state-warn-fg)]" />
                  <span>完全访问</span>
                </div>
                <span className="text-muted-foreground text-[11px] leading-tight">
                  全自动放行执行，不弹确认抽屉
                </span>
              </div>
            </SelectItem>
          </SelectContent>
        </Select>
        <span className="composer-spacer" />
        {isRunning ? (
          <ComposerPrimitive.Cancel className="send-btn stop" title="停止生成">
            <Square size={14} fill="currentColor" />
          </ComposerPrimitive.Cancel>
        ) : (
          <ComposerPrimitive.Send className="send-btn" title="发送">
            <Send size={15} />
          </ComposerPrimitive.Send>
        )}
      </div>
    </ComposerPrimitive.Root>
  );
}
