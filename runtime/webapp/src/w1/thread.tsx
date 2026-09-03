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
import { FolderOpen, PanelLeft, PanelRight, Send, ShieldAlert, Square } from "lucide-react";
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
      : JSON.stringify(req.args, null, 2).slice(0, 600);
  return (
    <div
      className="notice-warn flex flex-col gap-2 shadow-sm"
      style={{
        background: "var(--state-warn-bg)",
        borderColor: "var(--state-warn-border)",
        color: "var(--state-warn-fg)",
      }}
      data-slot="approval-card"
      data-approval-id={req.approval_id}
      data-status={req.status}
    >
      <div className="flex items-center gap-2">
        <ShieldAlert className="size-4" style={{ color: "var(--state-warn-fg)" }} />
        <span className="text-[13px] font-semibold" style={{ color: "var(--state-warn-fg)" }}>
          工具调用审批
        </span>
        <span
          className="rounded border px-1.5 py-0.5 font-mono text-[11px]"
          style={{
            borderColor: "var(--state-warn-border)",
            background: "color-mix(in srgb, var(--state-warn-bg) 70%, #000)",
            color: "var(--state-warn-fg)",
          }}
        >
          {req.capability}
        </span>
        <span className="text-[11px] opacity-80">
          风险:需审批 · 单据 {req.approval_id.slice(-6)}
        </span>
      </div>
      {req.args != null ? (
        <pre className="bg-card/70 mt-1 max-h-32 overflow-auto rounded-md border p-2 font-mono text-[11px] leading-relaxed text-foreground">
          {argsText}
        </pre>
      ) : null}
      {req.status === "waiting" ? (
        <div className="mt-2 flex items-center gap-2">
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
            className="text-destructive hover:bg-destructive/10"
            onClick={() => {
              setBusy(true);
              void onRespond(req.approval_id, "deny");
            }}
          >
            拒绝
          </Button>
          <span className="text-[11px] opacity-80">
            批准后工具立即执行;5 分钟未裁决自动过期
          </span>
        </div>
      ) : (
        <div className="mt-1 text-[12px] font-medium opacity-90">
          {req.status === "approved"
            ? "✓ 已批准——工具执行中/已完成,结果将回喂模型"
            : "✕ 已拒绝——已告知模型本次调用被用户取消"}
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
