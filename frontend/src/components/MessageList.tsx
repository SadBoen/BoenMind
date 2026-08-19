import { useEffect, useRef } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Message } from "../hooks/useChat";

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
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{m.text}</ReactMarkdown>
            ) : (
              <div className="msg-text">{m.text}</div>
            )}
            {m.pending && <span className="msg-cursor">▌</span>}
          </div>
        </div>
      ))}
    </div>
  );
}
