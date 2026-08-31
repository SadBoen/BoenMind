// W1 对话区:assistant-ui 原语组合(Thread/Message/Composer)
// 合约映射见 milestones/W1-implementation-spec.md §5
import {
  ComposerPrimitive,
  MessagePrimitive,
  ThreadPrimitive,
  useAuiState,
} from "@assistant-ui/react";
import type { ComponentPropsWithoutRef } from "react";

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
  return (
    <MessagePrimitive.Root className="msg assistant">
      <div className="model-tag">BoenMind Agent</div>
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
          ⚙ deepseek-v4-flash
        </span>
        <span className="tool-chip disabled">🏠 Home</span>
        <span className="composer-spacer" />
        <SendButton />
        {isRunning ? <span className="tool-chip disabled">生成中…</span> : null}
      </div>
    </ComposerPrimitive.Root>
  );
}

function SendButton(props: ComponentPropsWithoutRef<"button">) {
  return (
    <ComposerPrimitive.Send
      {...(props as object)}
      className="send-btn"
      title="发送"
    >
      ➤
    </ComposerPrimitive.Send>
  );
}
