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
import { PanelLeft, PanelRight, Send, ShieldAlert, Square } from "lucide-react";
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
import { ContextView } from "./context";
import { useBoenmindApprovals, type ApprovalRequest } from "./runtime";

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
      : JSON.stringify(req.args, null, 2).slice(0, 600);
  return (
    <div
      className="rounded-xl border border-amber-300 bg-amber-50 p-3 shadow-sm dark:bg-amber-950/30"
      data-slot="approval-card"
      data-approval-id={req.approval_id}
      data-status={req.status}
    >
      <div className="flex items-center gap-2">
        <ShieldAlert className="size-4 text-amber-600" />
        <span className="text-[13px] font-semibold text-amber-800 dark:text-amber-200">
          工具调用审批
        </span>
        <span className="rounded border border-amber-400 bg-amber-100 px-1.5 py-0.5 font-mono text-[10.5px] text-amber-800 dark:bg-amber-900/50 dark:text-amber-200">
          {req.capability}
        </span>
        <span className="text-[11px] text-muted-foreground">
          风险:需审批 · 单据 {req.approval_id.slice(-6)}
        </span>
      </div>
      {req.args != null ? (
        <pre className="bg-card mt-2 max-h-32 overflow-auto rounded-md border p-2 font-mono text-[11px] leading-relaxed">
          {argsText}
        </pre>
      ) : null}
      {req.status === "waiting" ? (
        <div className="mt-2.5 flex items-center gap-2">
          <Button
            size="sm"
            disabled={busy}
            data-slot="approval-approve"
            onClick={() => {
              setBusy(true);
              void onRespond(req.approval_id, "approve");
            }}
          >
            批准执行(单次)
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={busy}
            data-slot="approval-deny"
            className="text-red-600 hover:text-red-700"
            onClick={() => {
              setBusy(true);
              void onRespond(req.approval_id, "deny");
            }}
          >
            拒绝
          </Button>
          <span className="text-[11px] text-muted-foreground">
            批准后工具立即执行;5 分钟未裁决自动过期
          </span>
        </div>
      ) : (
        <div className="mt-2 text-[12.5px] font-medium text-muted-foreground">
          {req.status === "approved"
            ? "✓ 已批准——工具执行中/已完成,结果将回喂模型"
            : "✕ 已拒绝——已告知模型本次调用被用户取消"}
        </div>
      )}
    </div>
  );
}

// 小型本地 Button(避免从 w2 引入造成循环依赖;size/variant 仅作兼容占位)
function Button({
  children,
  className = "",
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  children: React.ReactNode;
  size?: string;
  variant?: string;
}) {
  return (
    <button
      className={
        "bg-primary text-primary-foreground hover:bg-primary/90 inline-flex h-8 items-center justify-center gap-1.5 rounded-md px-3 text-[12.5px] font-medium transition-colors disabled:opacity-50 " +
        className
      }
      {...rest}
    >
      {children}
    </button>
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
      <div className="text">
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

function Composer() {
  const isRunning = useAuiState((s) => s.thread.isRunning);
  const [model, setModel] = useState("…");
  // W6:对话级模型选择——候选 = 各提供商「常用」并集(设置→模型 勾选);
  // 选择持久化 localStorage,随每条消息发给后端,中途切换下一条即生效。
  const [modelGroups, setModelGroups] = useState<Array<{ provider: string; models: string[] }>>([]);
  const [selModel, setSelModel] = useState<string>(
    () => localStorage.getItem("bm_active_model") || "",
  );
  const [roles, setRoles] = useState<Array<{ id: string; name: string }>>([]);
  const [activeRole, setActiveRole] = useState<string>(
    () => localStorage.getItem("bm_active_role") || "",
  );

  const loadModels = () => {
    fetch("/admin/providers")
      .then((r) => r.json())
      .then((d) => {
        const list: Array<{ provider: string; models: string[] }> = (d?.providers ?? [])
          .filter((p: any) => (p.modelsCommon ?? []).length > 0)
          .map((p: any) => ({ provider: p.name, models: p.modelsCommon as string[] }));
        setModelGroups(list);
        // 已选模型不在候选集(被取消勾选/删除)→ 回落服务器默认
        const all = new Set(list.flatMap((g: any) => g.models));
        const cur = localStorage.getItem("bm_active_model") || "";
        if (cur && !all.has(cur)) {
          localStorage.removeItem("bm_active_model");
          setSelModel("");
        }
      })
      .catch(() => {});
  };

  const loadRoles = () => {
    fetch("/admin/roles")
      .then((r) => r.json())
      .then((d) => {
        if (d.roles && Array.isArray(d.roles)) {
          setRoles(d.roles);
          const current = localStorage.getItem("bm_active_role");
          if (!current || !d.roles.some((r: any) => r.id === current)) {
            const next = d.active_id || d.roles[0]?.id || "assistant";
            setActiveRole(next);
            localStorage.setItem("bm_active_role", next);
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
    window.addEventListener("bm-roles-changed", loadRoles);
    window.addEventListener("bm-providers-changed", loadModels);
    return () => {
      window.removeEventListener("bm-roles-changed", loadRoles);
      window.removeEventListener("bm-providers-changed", loadModels);
    };
  }, []);

  const handleRoleChange = (newRoleId: string) => {
    setActiveRole(newRoleId);
    localStorage.setItem("bm_active_role", newRoleId);
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
            <SelectContent className="rounded-lg">
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
            if (val) localStorage.setItem("bm_active_model", val);
            else localStorage.removeItem("bm_active_model");
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
          <SelectContent className="rounded-lg">
            <SelectItem value="__default__">⚙ 默认 {model}</SelectItem>
            {modelGroups.map((g) => (
              <SelectGroup key={g.provider}>
                <SelectLabel>{g.provider}</SelectLabel>
                {g.models.map((m) => (
                  <SelectItem key={m} value={m}>
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
        <span className="tool-chip disabled">🏠 Home</span>
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
