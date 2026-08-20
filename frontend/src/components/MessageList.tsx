import { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Message, ToolBlock } from "../hooks/useChat";

function ToolCallCard({ block }: { block: ToolBlock }) {
  const [expanded, setExpanded] = useState(false);
  const done = block.output !== undefined;
  return (
    <div className={`tool-card${done ? (block.isError ? " tool-card-error" : " tool-card-done") : ""}`}>
      <button
        className="tool-card-head"
        onClick={() => setExpanded((e) => !e)}
        type="button"
      >
        <span className="tool-card-status">{done ? (block.isError ? "✕" : "✓") : "…"}</span>
        <span className="tool-card-name">{block.name ?? "tool"}</span>
        {done && <span className="tool-card-hint">{block.isError ? "失败" : "完成"}</span>}
      </button>
      {expanded && (
        <div className="tool-card-detail">
          {block.arguments && (
            <pre className="tool-card-args">
              {(() => {
                try {
                  return JSON.stringify(JSON.parse(block.arguments), null, 2);
                } catch {
                  return block.arguments;
                }
              })()}
            </pre>
          )}
          {done && (
            <pre className={`tool-card-output${block.isError ? " is-error" : ""}`}>{block.output}</pre>
          )}
        </div>
      )}
    </div>
  );
}

export default function MessageList({ messages }: { messages: Message[] }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.scrollTo({ top: ref.current.scrollHeight });
  }, [messages]);
  return (
    <div className="chat-messages" ref={ref}>
      {messages.length === 0 && (
        <div className="chat-empty">开始对话吧 —— 在下方输入消息</div>
      )}
      {messages.map((m) => (
        <div key={m.id} className={`msg msg-${m.role}`}>
          <div className="msg-avatar">{m.role === "user" ? "我" : "B"}</div>
          <div className="msg-body">
            {m.role === "assistant" ? (
              m.text ? (
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{m.text}</ReactMarkdown>
              ) : null
            ) : (
              <div className="msg-text">{m.text}</div>
            )}
            {m.blocks?.map((b, i) => (
              <ToolCallCard key={i} block={b} />
            ))}
            {m.pending && <span className="msg-cursor">▌</span>}
          </div>
        </div>
      ))}
    </div>
  );
}