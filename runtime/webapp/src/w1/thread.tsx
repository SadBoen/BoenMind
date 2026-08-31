// W1 对话区:assistant-ui 原语组合(Thread/Message/Composer)
// 合约映射见 milestones/W1-implementation-spec.md §5
import {
  ComposerPrimitive,
  MessagePrimitive,
  ThreadPrimitive,
  useAuiState,
} from "@assistant-ui/react";
import { Send, Square } from "lucide-react";
import { useEffect, useState } from "react";

export function Thread() {
  const isEmpty = useAuiState((s) => s.thread.isEmpty);
  return (
    <div className="chat">
      <div className="chat-head">
        <span className="name">BoenMind 对话</span>
        <span className="badge">self-hosted</span>
      </div>
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
          <ThreadPrimitive.ViewportFooter className="composer-dock">
            <Composer />
          </ThreadPrimitive.ViewportFooter>
        </ThreadPrimitive.Viewport>
      </ThreadPrimitive.Root>
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
  const isRunning = useAuiState((s) => s.message.isRunning);
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
  // 模型名动态取自服务器 /v1/models(单一配置模型,W1 口径)
  useEffect(() => {
    fetch("/v1/models")
      .then((r) => r.json())
      .then((v) => setModel(v?.data?.[0]?.id ?? "?"))
      .catch(() => setModel("?"));
  }, []);
  return (
    <ComposerPrimitive.Root className="composer">
      <ComposerPrimitive.Input
        className="composer-input"
        placeholder="Message BoenMind…"
        rows={2}
        autoFocus
      />
      <div className="composer-toolbar">
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
