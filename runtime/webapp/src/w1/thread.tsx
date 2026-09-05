// W1 对话区:assistant-ui 原语组合(Thread/Message/Composer)
// 合约映射见 milestones/W1-implementation-spec.md §5
// W5:对话区页签(对话/上下文)——上下文页 = 每次模型调用请求快照透视
// W4b:对话内审批卡片(ApprovalCards)——工具调用需审批时在流内渲染
import {
  ComposerPrimitive,
  MessagePrimitive,
  ThreadPrimitive,
  useAuiState,
} from "@assistant-ui/react";
import {
  FolderOpen,
  PanelLeft,
  PanelRight,
  Send,
  ShieldAlert,
  ShieldCheck,
  Square,
  CheckCircle2,
  XCircle,
  Clock,
  ChevronUp,
  ChevronDown,
  ChevronRight,
  Zap,
  ListOrdered,
  Code2,
  X,
  Search,
  Terminal,
  Brain,
  FileText,
  FileCode,
  FileJson,
  FileImage,
  File,
  Sparkles,
  Bot,
  User,
} from "lucide-react";
import { useEffect, useState } from "react";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ContextView } from "./context";
import { useBoenmindApprovals, type ApprovalRequest } from "./runtime";
import { api, type WorkspaceEntry } from "@/w2/api";
import { storage, STORAGE_KEYS, type PermissionMode } from "@/lib/storage";
import { cn } from "@/lib/utils";
import { BM_EVENTS, emit } from "../lib/bus";

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
          <ThreadPrimitive.Viewport className="thread-viewport">
            {isEmpty ? (
              <div className="welcome">
                <div className="logo">B</div>
                <h1>个人生态的 AI Runtime</h1>
                <p>自研 Agent 已就绪——直接输入,流式回复。</p>
              </div>
            ) : (
              <ThreadPrimitive.Messages>
                {({ message }) =>
                  message.role === "user" ? <UserMessage /> : <AssistantMessage />
                }
              </ThreadPrimitive.Messages>
            )}
          </ThreadPrimitive.Viewport>
          <div className="composer-dock">
            <div className="relative mx-auto w-full max-w-[820px]">
              <ApprovalDrawer />
              <Composer />
            </div>
          </div>
        </ThreadPrimitive.Root>
      )}
    </div>
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
  return (
    <MessagePrimitive.Root className="msg user">
      <div className="msg-header">
        <User size={13} className="text-primary" />
        <span>我</span>
      </div>
      <div className="content">
        <MessagePrimitive.Parts>
          {({ part }) =>
            part.type === "text" ? (
              <span key={part.text.length}>{part.text}</span>
            ) : null
          }
        </MessagePrimitive.Parts>
      </div>
    </MessagePrimitive.Root>
  );
}

// 根据文件名后缀返回对应的专属图标
function getFileIcon(filename: string) {
  const lower = filename.toLowerCase();
  if (lower.endsWith(".ts") || lower.endsWith(".tsx") || lower.endsWith(".js") || lower.endsWith(".jsx") || lower.endsWith(".rs") || lower.endsWith(".py") || lower.endsWith(".go") || lower.endsWith(".c") || lower.endsWith(".cpp")) {
    return <FileCode className="size-3.5 text-sky-500 shrink-0" />;
  }
  if (lower.endsWith(".json") || lower.endsWith(".yaml") || lower.endsWith(".yml") || lower.endsWith(".toml") || lower.endsWith(".xml")) {
    return <FileJson className="size-3.5 text-amber-500 shrink-0" />;
  }
  if (lower.endsWith(".md") || lower.endsWith(".txt") || lower.endsWith(".doc") || lower.endsWith(".pdf")) {
    return <FileText className="size-3.5 text-indigo-500 shrink-0" />;
  }
  if (lower.endsWith(".png") || lower.endsWith(".jpg") || lower.endsWith(".svg") || lower.endsWith(".webp") || lower.endsWith(".gif")) {
    return <FileImage className="size-3.5 text-emerald-500 shrink-0" />;
  }
  if (lower.endsWith(".sh") || lower.endsWith(".bat") || lower.endsWith(".ps1")) {
    return <Terminal className="size-3.5 text-rose-500 shrink-0" />;
  }
  return <File className="size-3.5 text-muted-foreground shrink-0" />;
}

// 解析助手消息中的工具调用与普通文本，并进行连续折叠聚合
type ParsedBlock =
  | { type: "text"; text: string }
  | { type: "thinking"; text: string }
  | { type: "tool_group"; tools: Array<{ raw: string; name: string; target?: string }> };

function parseAssistantText(raw: string): ParsedBlock[] {
  if (!raw) return [];

  // 工具调用标记匹配: [调用 tool_name] 或类似格式
  const lines = raw.split("\n");
  const blocks: ParsedBlock[] = [];
  let curText = "";
  let curTools: Array<{ raw: string; name: string; target?: string }> = [];

  const flushText = () => {
    if (curText) {
      blocks.push({ type: "text", text: curText });
      curText = "";
    }
  };

  const flushTools = () => {
    if (curTools.length > 0) {
      blocks.push({ type: "tool_group", tools: [...curTools] });
      curTools = [];
    }
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const toolMatch = line.match(/^\[调用\s+([a-zA-Z0-9_\-\.:]+)(?:\s*(.*?))?\]$/);
    if (toolMatch) {
      flushText();
      const name = toolMatch[1];
      const target = toolMatch[2] || "";
      curTools.push({ raw: line, name, target });
    } else {
      flushTools();
      curText += (curText ? "\n" : "") + line;
    }
  }

  flushTools();
  flushText();

  return blocks;
}

function ToolGroupCard({
  group,
  isRunning,
}: {
  group: { tools: Array<{ raw: string; name: string; target?: string }> };
  isRunning?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const count = group.tools.length;

  // 统计工具类型
  const searchCount = group.tools.filter((t) => t.name.includes("search") || t.name.includes("find")).length;
  const readCount = group.tools.filter((t) => t.name.includes("read")).length;
  const execCount = group.tools.filter((t) => t.name.includes("exec") || t.name.includes("bash")).length;

  const summaryParts: string[] = [];
  if (searchCount > 0) summaryParts.push(`${searchCount} 搜索`);
  if (readCount > 0) summaryParts.push(`${readCount} 读取`);
  if (execCount > 0) summaryParts.push(`${execCount} 终端`);
  const summaryText = summaryParts.length > 0 ? summaryParts.join("，") : `${count} 个操作`;

  return (
    <div className="tool-group-card" data-slot="tool-group">
      <div className="tool-group-header" onClick={() => setOpen(!open)}>
        {open ? <ChevronDown size={14} className="text-muted-foreground" /> : <ChevronRight size={14} className="text-muted-foreground" />}
        <div className="tool-group-title">
          <Search size={14} className="text-blue-500" />
          <span>查阅 · {summaryText}</span>
        </div>
        <span className="tool-group-summary">{open ? "收起" : "展开详情"}</span>
      </div>
      {open ? (
        <div className="tool-group-body">
          {group.tools.map((t, idx) => {
            const isRead = t.name.includes("read");
            const isExec = t.name.includes("exec");
            return (
              <div key={idx} className="tool-step-item">
                {isExec ? (
                  <Terminal size={13} className="text-emerald-500 shrink-0" />
                ) : isRead ? (
                  getFileIcon(t.target || t.name)
                ) : (
                  <Search size={13} className="text-blue-500 shrink-0" />
                )}
                <span className="font-semibold text-foreground">{t.name}</span>
                {t.target ? <span className="tool-step-cmd">{t.target}</span> : null}
              </div>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

function AssistantMessage() {
  const isRunning = useAuiState((s) => s.thread.isRunning);
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
            const blocks = parseAssistantText(part.text);
            return (
              <div className="flex flex-col gap-1.5" key={part.text.length}>
                {blocks.map((b, idx) => {
                  if (b.type === "tool_group") {
                    return <ToolGroupCard key={idx} group={b} isRunning={isRunning} />;
                  }
                  return (
                    <span key={idx} className="leading-relaxed text-[13.5px]">
                      {b.text}
                    </span>
                  );
                })}
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
      .catch(() => {});
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
        }
      })
      .catch(() => {});
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
      .catch(() => {});
  };

  useEffect(() => {
    fetch("/v1/models")
      .then((r) => r.json())
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
        {roles.length > 0 ? (
          // W6 反馈:去「角色:」文字标签;W7 反馈:换主题化下拉(弹出层跟皮肤走,不再是直角原生框)
          <Select value={activeRole} onValueChange={handleRoleChange}>
            <SelectTrigger
              size="sm"
              className="bg-muted/60 h-7 border px-2 text-[12px] font-medium"
              title="切换当前会话角色"
              data-slot="role-select"
            >
              <SelectValue />
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
        <span className="tool-chip disabled">📎 附件</span>
        {/* W6 对话级模型选择:候选 = 各提供商「常用」并集;中途切换下一条生效。
            Radix 不允许空值 item,服务器默认用哨兵 __default__ 表示(=不传 model) */}
        <Select
          value={selModel || "__default__"}
          onValueChange={(v) => {
            const val = v === "__default__" ? "" : v;
            setSelModel(val);
            if (val) storage.set(STORAGE_KEYS.ACTIVE_MODEL, val);
            else storage.remove(STORAGE_KEYS.ACTIVE_MODEL);
          }}
        >
          <SelectTrigger
            size="sm"
            className="bg-muted/60 h-7 border px-2 text-[12px] font-medium"
            title="切换对话模型:下一条消息即生效,无需新开会话;候选在 设置→模型 勾选「常用」"
            data-slot="model-select"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent
            className="rounded-lg"
            side="top"
            position="popper"
          >
            {/* W7 反馈:上拉;去掉「默认」字样;Provider 名大一号、模型名小一号
                且缩进;在用项打钩(钩移到名字前面) */}
            <SelectItem
              value="__default__"
              className="text-[12px] pl-2 [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
            >
              ⚙ {model}
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
        {/* W8:工作目录选择(替换原 🏠 Home 占位)。上拉菜单排版参考用户样张:
            条目两行 = 名称 + 路径;「跟随默认」= 不绑定(__auto__ 哨兵,Radix 禁空值) */}
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
            size="sm"
            className="bg-muted/60 h-7 max-w-44 border px-2 text-[12px] font-medium"
            title="切换本对话工作目录:下一条消息即生效;目录在 设置→常规 维护"
            data-slot="workspace-select"
          >
            <FolderOpen className="size-3.5 opacity-70" />
            <span className="truncate">
              {workspaces.find((w) => w.id === selWorkspace)?.name ?? "默认工作区"}
            </span>
          </SelectTrigger>
          <SelectContent className="rounded-lg" side="top" position="popper">
            <SelectItem
              value="__auto__"
              className="text-[12px] pl-2 [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
            >
              默认(不绑定工作目录)
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

        {/* 权限模式选择上拉菜单 (图 1 标杆交互):
            变更前确认(默认) | 计划模式 | 完全访问(YOLO) */}
        <Select
          value={permMode}
          onValueChange={(v) => {
            const val = v as PermissionMode;
            setPermMode(val);
            storage.set(STORAGE_KEYS.PERMISSION_MODE, val);
          }}
        >
          <SelectTrigger
            size="sm"
            className="bg-muted/60 h-7 border px-2 text-[12px] font-medium"
            title="切换权限模式:变更前确认(默认) / 计划模式 / 完全访问(YOLO免审批)"
            data-slot="permission-select"
          >
            {permMode === "yolo" ? (
              <>
                <Zap className="size-3.5 text-[var(--state-warn-fg)]" />
                <span>完全访问</span>
              </>
            ) : permMode === "plan" ? (
              <>
                <ListOrdered className="size-3.5 text-blue-500" />
                <span>计划模式</span>
              </>
            ) : (
              <>
                <ShieldCheck className="size-3.5 text-[var(--state-success-fg)]" />
                <span>变更前确认</span>
              </>
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
