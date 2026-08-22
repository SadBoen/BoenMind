import { useEffect, useRef, useState } from "react";
import { ScrollIndicators } from "../components/ScrollIndicators";
import { MarkdownView } from "../lib/markdown";
import { formatBytes, formatFullDate, formatTime } from "../lib/format";
import { IconBot, IconBrain, IconChevron, IconCopy, IconEdit, IconEmptyChat, IconRetry, IconUser } from "../lib/icons";
import { useStore } from "../store";
import { toast } from "../lib/toast";
import type { ChatMessage, ToolCall } from "../types";

function thinkOpen(_msg: ChatMessage, streaming: boolean, last: boolean, pref: "auto" | "expanded" | "hidden", override?: boolean) {
  if (pref === "hidden") return false;
  if (override !== undefined) return override;
  if (pref === "expanded") return true;
  return streaming && last;
}

function ToolRow({ tool }: { tool: ToolCall }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={`tool-card${tool.status === "running" ? " is-running" : ""}`}>
      <button type="button" className="meta-row" onClick={() => setOpen((v) => !v)}>
        <IconChevron style={{ transform: open ? "rotate(0deg)" : "rotate(-90deg)" }} />
        {tool.name} · {tool.summary}
      </button>
      {open && tool.detail && <div className="think-body">{tool.detail}</div>}
    </div>
  );
}

function ThinkCard({
  msg,
  streaming,
  last,
}: {
  msg: ChatMessage;
  streaming: boolean;
  last: boolean;
}) {
  const { state, dispatch } = useStore();
  if (!msg.thinking && !(streaming && last && state.reasoning !== "off")) return null;
  const override = state.thinkingOverrides[msg.id];
  const open = thinkOpen(msg, streaming && last, last, state.settings.thinkingDisplay, override);
  const done = !(streaming && last);
  const secs = done && state.streamStartedAt ? Math.max(1, Math.round((msg.createdAt + 1000 - (state.streamStartedAt ?? msg.createdAt)) / 1000)) : 0;
  const summary = (msg.thinking ?? "").split("\n")[0].slice(0, 48);
  return (
    <div>
      <button
        type="button"
        className="think-row"
        onClick={() => dispatch({ type: "override-thinking", id: msg.id, open: !open })}
      >
        <IconBrain />
        {streaming && last ? "思考中…" : `思考完成${secs ? ` · ${secs}s` : ""}`}
        {done && summary ? ` · ${summary}` : ""}
        <IconChevron style={{ marginLeft: "auto", transform: open ? "rotate(0deg)" : "rotate(-90deg)" }} />
      </button>
      {open && (
        streaming && last ? (
          <pre className="think-body">{msg.thinking || "…"}</pre>
        ) : (
          <MarkdownView>{msg.thinking ?? ""}</MarkdownView>
        )
      )}
    </div>
  );
}

export function MessageList() {
  const { state, dispatch, activeMessages } = useStore();
  const sid = state.activeSessionId;
  const streaming = state.streaming;
  const scrollRef = useRef<HTMLDivElement>(null);
  // 是否吸附底部：用户上翻阅读时暂停自动跟随，滚回底部恢复。
  const stickRef = useRef(true);

  useEffect(() => {
    const el = scrollRef.current;
    if (el && stickRef.current) el.scrollTop = el.scrollHeight;
  }, [activeMessages, streaming]);

  if (!sid) {
    return (
      <div className="empty">
        <IconEmptyChat />
        <div>还没有会话 —— 在左侧新建一个开始。</div>
      </div>
    );
  }
  if (activeMessages.length === 0) {
    return (
      <div className="empty">
        <IconEmptyChat />
        <div>这个会话还没有消息 —— 在下方输入第一条。</div>
      </div>
    );
  }

  return (
    <div className="chat-stage">
    <div
      className="chat-scroll"
      ref={scrollRef}
      onScroll={() => {
        const el = scrollRef.current;
        if (!el) return;
        stickRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
      }}
    >
      <div className="chat-inner">
        {activeMessages.map((m, i) => {
          const last = i === activeMessages.length - 1;
          if (m.compressed) {
            return (
              <div key={m.id} className="compress">
                上下文自动压缩
              </div>
            );
          }
          if (m.role === "user") {
            return (
              <div key={m.id} className="msg is-user" data-mid={m.id} data-role="user" data-preview={m.content}>
                <div className="avatar" aria-hidden="true">
                  <IconUser />
                </div>
                <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end" }}>
                  <div className="bubble">{m.content}</div>
                  {m.attachments?.map((a) => (
                    <span key={a.id} className="attach-chip">
                      {a.name} · {formatBytes(a.size)}
                    </span>
                  ))}
                  <div className="msg-actions">
                    <button
                      type="button"
                      className="icon-btn"
                      aria-label="编辑"
                      onClick={() => dispatch({ type: "edit-user", id: m.id, content: m.content })}
                    >
                      <IconEdit />
                    </button>
                  </div>
                </div>
              </div>
            );
          }
          return (
            <div key={m.id} className={`msg${streaming && last ? " live" : ""}`}>
              <div className="avatar" aria-hidden="true">
                <IconBot />
              </div>
              <div className="prose-block">
                <div className="msg-role">
                  <b>{m.model ?? state.model}</b>
                  <span title={formatFullDate(m.createdAt)}>{formatTime(m.createdAt)}</span>
                </div>
                <ThinkCard msg={m} streaming={streaming} last={last} />
                {m.tools?.map((t) => (
                  <ToolRow key={t.id} tool={t} />
                ))}
                <MarkdownView streaming={streaming && last}>{m.content}</MarkdownView>
                {streaming && last && <span className="caret" />}
                <div className="msg-actions">
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label="复制"
                    onClick={async () => {
                      await navigator.clipboard.writeText(m.content);
                      toast.success("已复制");
                    }}
                  >
                    <IconCopy />
                  </button>
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label="重试"
                    onClick={() => {
                      const prev = activeMessages.slice(0, i).reverse().find((x) => x.role === "user");
                      if (prev) dispatch({ type: "retry", userId: prev.id });
                    }}
                  >
                    <IconRetry />
                  </button>
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
    <ScrollIndicators containerRef={scrollRef} />
    </div>
  );
}
