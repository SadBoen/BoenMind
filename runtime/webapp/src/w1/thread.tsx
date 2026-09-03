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
  Square,
  CheckCircle2,
  XCircle,
  Clock,
  ChevronUp,
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
import { storage, STORAGE_KEYS } from "@/lib/storage";

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
          <ApprovalCards />
          <div className="composer-dock">
            <Composer />
          </div>
        </ThreadPrimitive.Root>
      )}
    </div>
  );
}

// W4b 对话内审批卡片:waiting 态可裁决;批准/拒绝后状态固化并回传 /rpc
function ApprovalCards() {
  const { pendingApprovals, respondApproval } = useBoenmindApprovals();
  if (pendingApprovals.length === 0) return null;
  return (
    <div className="flex flex-col gap-2 px-4 py-1" data-slot="approval-cards">
      {pendingApprovals.map((a) => (
        <ApprovalCard
          key={a.approval_id}
          req={a}
          onRespond={respondApproval}
        />
      ))}
    </div>
  );
}

function ApprovalCard({
  req,
  onRespond,
}: {
  req: ApprovalRequest;
  onRespond: (id: string, d: "approve" | "deny") => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const argsText =
    req.args == null
      ? "(参数见上文模型调用)"
      : JSON.stringify(req.args, null, 2).slice(0, 800);

  const handleAction = async (decision: "approve" | "deny") => {
    if (busy) return;
    setBusy(true);
    try {
      await onRespond(req.approval_id, decision);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="group relative my-3 flex flex-col gap-3 rounded-2xl border border-border/80 bg-card/75 p-4 shadow-lg backdrop-blur-xl transition-all hover:border-border"
      data-slot="approval-card"
      data-approval-id={req.approval_id}
      data-status={req.status}
    >
      {/* 头部：标题、能力名徽标、状态与单据号 */}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2.5">
          <div className="flex size-7 items-center justify-center rounded-lg bg-amber-500/15 text-amber-600 dark:text-amber-400">
            <ShieldAlert className="size-4" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="text-[13.5px] font-semibold text-foreground tracking-tight">
                权限与工具调用请求
              </span>
              <Badge variant="outline" className="border-amber-500/30 bg-amber-500/10 font-mono text-[11px] text-amber-700 dark:text-amber-300">
                {req.capability}
              </Badge>
            </div>
            <p className="text-[11.5px] text-muted-foreground mt-0.5">
              该操作具有外部副作用或修改权限，需人工确认裁决 · 单据 #{req.approval_id.slice(-6)}
            </p>
          </div>
        </div>

        {req.status === "waiting" ? (
          <Badge variant="secondary" className="flex items-center gap-1 font-mono text-[11px] text-muted-foreground">
            <Clock className="size-3 animate-spin text-amber-500" />
            等待裁决 (5m)
          </Badge>
        ) : null}
      </div>

      {/* 参数预览区域：精致代码块卡片 */}
      {req.args != null ? (
        <div className="relative overflow-hidden rounded-xl border border-border/60 bg-muted/40 p-2.5">
          <div className="mb-1.5 flex items-center justify-between text-[10.5px] font-medium text-muted-foreground uppercase tracking-wider">
            <span>调用参数 (Payload)</span>
            <span className="font-mono text-[10px] opacity-70">JSON</span>
          </div>
          <pre className="max-h-36 overflow-auto font-mono text-[11.5px] leading-relaxed text-foreground/90 selection:bg-primary/20">
            {argsText}
          </pre>
        </div>
      ) : null}

      {/* 底部操作区：仿 ZCode 上拉选项菜单交互 */}
      {req.status === "waiting" ? (
        <div className="mt-1 flex flex-wrap items-center justify-between gap-3 border-t border-border/40 pt-3">
          {/* 左侧：直接主要操作 */}
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              disabled={busy}
              className="h-8 rounded-lg px-3.5 text-[12.5px] font-medium shadow-sm transition-transform active:scale-95"
              data-slot="approval-approve"
              onClick={() => void handleAction("approve")}
            >
              ✓ 允许本次执行
            </Button>

            {/* 上拉操作菜单 */}
            <Select
              onValueChange={(val) => {
                if (val === "approve") void handleAction("approve");
                else if (val === "deny") void handleAction("deny");
              }}
            >
              <SelectTrigger
                size="sm"
                disabled={busy}
                className="h-8 gap-1.5 rounded-lg border-border/80 bg-background/60 px-2.5 text-[12px] font-medium hover:bg-accent"
                title="选择裁决动作"
              >
                <span>更多选项</span>
              </SelectTrigger>
              <SelectContent side="top" align="start" className="rounded-xl border border-border/80 bg-popover/95 p-1 shadow-xl backdrop-blur-md">
                <SelectLabel className="px-2 py-1 text-[11px] font-semibold text-muted-foreground">
                  权限裁决策略
                </SelectLabel>
                <SelectItem
                  value="approve"
                  className="rounded-lg py-2 pl-2 text-[12px] font-medium focus:bg-primary/10 focus:text-primary"
                >
                  <div className="flex flex-col gap-0.5">
                    <span className="font-semibold text-foreground">批准本次调用 (Allow)</span>
                    <span className="text-[10.5px] text-muted-foreground">允许当前单据执行一次，随后继续对话</span>
                  </div>
                </SelectItem>
                <SelectItem
                  value="deny"
                  className="rounded-lg py-2 pl-2 text-[12px] font-medium text-destructive focus:bg-destructive/10 focus:text-destructive"
                >
                  <div className="flex flex-col gap-0.5">
                    <span className="font-semibold">拒绝本次调用 (Deny)</span>
                    <span className="text-[10.5px] opacity-80">终止当前操作并告知模型调用已被用户取消</span>
                  </div>
                </SelectItem>
              </SelectContent>
            </Select>

            <Button
              size="sm"
              variant="ghost"
              disabled={busy}
              data-slot="approval-deny"
              className="h-8 rounded-lg px-2.5 text-[12px] text-destructive hover:bg-destructive/10 hover:text-destructive"
              onClick={() => void handleAction("deny")}
            >
              拒绝
            </Button>
          </div>

          <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
            <span>支持对话内热审批</span>
            <span>·</span>
            <span>超时自动回滚</span>
          </div>
        </div>
      ) : (
        /* 终态提示 */
        <div
          className={cn(
            "flex items-center gap-2 rounded-xl border px-3 py-2 text-[12px] font-medium shadow-xs",
            req.status === "approved"
              ? "border-emerald-500/20 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
              : "border-destructive/20 bg-destructive/10 text-destructive",
          )}
        >
          {req.status === "approved" ? (
            <>
              <CheckCircle2 className="size-4 shrink-0 text-emerald-500" />
              <span>已批准执行 —— 工具结果已安全回喂给模型并归档</span>
            </>
          ) : (
            <>
              <XCircle className="size-4 shrink-0 text-destructive" />
              <span>已拒绝本次调用 —— 已通知模型操作取消并释放执行锁</span>
            </>
          )}
        </div>
      )}
    </div>
  );
}

function UserMessage() {
  return (
    <MessagePrimitive.Root className="msg user">
      <div className="bubble">
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

function AssistantMessage() {
  // 0.15:运行态在 thread 级(单会话口径,tag 显示全局即可)
  const isRunning = useAuiState((s) => s.thread.isRunning);
  return (
    <MessagePrimitive.Root className="msg assistant">
      <div className="model-tag">BoenMind Agent{isRunning ? " · 生成中" : ""}</div>
      {/* W8:空正文不出空气泡——text 为空返回 null,.text:empty 由 CSS 隐藏;
          停止/连接失败文本非空,照常显示 */}
      <div className="text">
        <MessagePrimitive.Parts>
          {({ part }) =>
            part.type === "text" && part.text ? (
              <span key={part.text.length}>{part.text}</span>
            ) : null
          }
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
    window.addEventListener("bm-roles-changed", loadRoles);
    window.addEventListener("bm-providers-changed", loadModels);
    window.addEventListener("bm-workspaces-changed", loadWorkspaces);
    return () => {
      window.removeEventListener("bm-roles-changed", loadRoles);
      window.removeEventListener("bm-providers-changed", loadModels);
      window.removeEventListener("bm-workspaces-changed", loadWorkspaces);
    };
  }, []);

  const handleRoleChange = (newRoleId: string) => {
    setActiveRole(newRoleId);
    storage.set(STORAGE_KEYS.ACTIVE_ROLE, newRoleId);
    // 切换角色时清空旧会话并重开，使新角色的 system_prompt 立即绑定
    window.dispatchEvent(new CustomEvent("bm-chat-new"));
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
              className="bg-muted/60 h-7 rounded-lg border px-2 text-[11.5px] font-medium"
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
            className="bg-muted/60 h-7 rounded-lg border px-2 text-[11.5px] font-medium"
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
            className="bg-muted/60 h-7 max-w-44 rounded-lg border px-2 text-[11.5px] font-medium"
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
