// W1 对话区:assistant-ui 原语组合(Thread/Message/Composer)
// 合约映射见 milestones/W1-implementation-spec.md §5
// W5:对话区页签(对话/上下文)——上下文页 = 每次模型调用请求快照透视
import {
  ComposerPrimitive,
  MessagePrimitive,
  ThreadPrimitive,
  useAuiState,
} from "@assistant-ui/react";
import { Send, Square } from "lucide-react";
import { useEffect, useState } from "react";
import { ContextView } from "./context";

export function Thread() {
  const isEmpty = useAuiState((s) => s.thread.isEmpty);
  // W5 页签:对话 = 聊天;上下文 = 请求快照透视(dsh-context 同款布局理念)
  const [tab, setTab] = useState<"chat" | "ctx">("chat");
  const tabCls = (active: boolean) =>
    "rounded-full px-2.5 py-0.5 text-[12px] transition-colors " +
    (active
      ? "bg-primary text-primary-foreground"
      : "text-muted-foreground hover:bg-muted");
  return (
    <div className="chat">
      <div className="chat-head">
        <span className="name">BoenMind 对话</span>
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
        <span className="badge">self-hosted</span>
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
            <Composer />
          </div>
        </ThreadPrimitive.Root>
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
  const [roles, setRoles] = useState<Array<{ id: string; name: string }>>([]);
  const [activeRole, setActiveRole] = useState<string>(
    () => localStorage.getItem("bm_active_role") || "",
  );

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

    loadRoles();
    window.addEventListener("bm-roles-changed", loadRoles);
    return () => window.removeEventListener("bm-roles-changed", loadRoles);
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
          <div className="flex items-center gap-1">
            <span className="text-[12px] text-muted-foreground ml-1">角色:</span>
            <select
              value={activeRole}
              onChange={(e) => handleRoleChange(e.target.value)}
              className="bg-muted/60 text-foreground hover:bg-muted focus:ring-ring h-7 rounded-md border px-2 text-[11.5px] font-medium outline-none transition-colors"
              title="切换当前会话角色"
              data-slot="role-select"
            >
              {roles.map((r) => (
                <option key={r.id} value={r.id}>
                  🎭 {r.name}
                </option>
              ))}
            </select>
          </div>
        ) : null}
        <span className="tool-chip disabled">📎 附件</span>
        <span className="tool-chip mono disabled" title="服务器配置模型(W1 固定)">
          ⚙ {model}
        </span>
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
