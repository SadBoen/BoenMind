/**
 * 消息渲染：用户气泡 + 助手（思考/工具执行过程折叠块 + 正式答复 Markdown）。
 * 对齐 pi-web 语义：绿条（工具调用）属于"过程"，收纳在思考/执行折叠块内，
 * 正式答复（纯文本）下方不再显示工具块。
 */
import { memo, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { Bot, Check, Copy, GitFork, User } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import type { Message } from "@/api/client";
import { useAppStore } from "@/stores/app-store";
import { ProcessBlock, parseThinkBlocks } from "./ThinkBlock";
import { ToolCallBlock, type ToolCallView } from "./ToolCallBlock";

export const MessageItem = memo(function MessageItem({
  message,
  streaming,
  streamingToolCalls,
}: {
  message: Message;
  streaming?: boolean;
  /** 流式中的工具调用（仅临时 assistant 消息传入，running 状态展示） */
  streamingToolCalls?: ToolCallView[];
}) {
  const isUser = message.role === "user";

  // 助手消息：解析 think 块（思考段 + 纯文本段）
  const segments = useMemo(() => {
    if (isUser) return null;
    return parseThinkBlocks(message.content);
  }, [isUser, message.content]);

  // 工具调用块：历史消息的固化记录在前，流式中的（running）在后
  const toolCalls = useMemo<ToolCallView[]>(() => {
    if (isUser) return [];
    const saved = (message.tool_calls ?? []).map((c) => ({
      tool_name: c.tool_name,
      args: c.args,
      is_error: c.is_error,
    }));
    return [...saved, ...(streamingToolCalls ?? [])];
  }, [isUser, message.tool_calls, streamingToolCalls]);

  if (isUser) {
    return (
      <div className="msg-enter flex justify-end gap-3">
        <div className="max-w-[85%] rounded-2xl rounded-br-sm bg-primary px-4 py-2.5 text-sm text-primary-foreground">
          <p className="whitespace-pre-wrap break-words">{message.content}</p>
        </div>
        <div className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-muted">
          <User size={14} />
        </div>
      </div>
    );
  }

  const thinks = segments?.filter((s) => s.type === "think") ?? [];
  const textParts = segments?.filter((s) => s.type === "text") ?? [];
  // 流式过程中（未闭合 think 或工具执行中）默认展开过程块
  const processOpen = streaming && (thinks.some((s) => s.open) || toolCalls.some((c) => c.running));
  const bodyText = textParts.map((s) => s.content).join("");

  return (
    <div className="msg-enter group flex gap-3">
      <div className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
        <Bot size={14} />
      </div>
      <div className="min-w-0 flex-1">
        {/* 过程区：思考内容 + 工具调用块（绿条归属这里，正式答复不显示） */}
        {(thinks.length > 0 || toolCalls.length > 0) && (
          <ProcessBlock
            thinks={thinks.map((s) => ({ content: s.content }))}
            toolCount={toolCalls.length > 0 ? toolCalls.length : undefined}
            defaultOpen={processOpen}
          >
            {toolCalls.map((c, i) => (
              // 名称+序号做 key：流式固化后索引位移不会把展开状态错挂到别的调用上
              <ToolCallBlock key={`${c.tool_name}-${i}`} call={c} />
            ))}
          </ProcessBlock>
        )}
        {/* 正式答复：纯文本（不含 think） */}
        {bodyText.trim() !== "" && (
          <div className={cn("prose prose-sm dark:prose-invert max-w-none break-words", streaming && "animate-pulse")}>
            <Markdown content={bodyText} />
          </div>
        )}
        {/* 答复末尾操作栏（2026-08-16 用户定调）：复制 + 分叉，悬停显示 */}
        {!streaming && bodyText.trim() !== "" && <MessageActions content={bodyText} messageId={message.id} />}
      </div>
    </div>
  );
});

/**
 * 答复末尾操作栏：复制（纯文本）+ 分叉（从该消息开新会话，历史复制到此处）。
 */
function MessageActions({ content, messageId }: { content: string; messageId: number }) {
  const { t } = useTranslation();
  const forkFromMessage = useAppStore((s) => s.forkFromMessage);
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(content);
    } catch {
      // IAB/非安全上下文兜底：临时 textarea + execCommand
      const ta = document.createElement("textarea");
      ta.value = content;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
    setCopied(true);
    toast.success(t("chat.message.copied"));
    window.setTimeout(() => setCopied(false), 1500);
  };

  const fork = async () => {
    try {
      await forkFromMessage(messageId);
      toast.success(t("chat.message.forked"));
    } catch (err) {
      toast.error(t("chat.message.forkFailed", { error: String(err) }));
    }
  };

  return (
    <div className="mt-1.5 flex items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
      <button
        type="button"
        onClick={() => void copy()}
        title={t("chat.message.copy")}
        className="flex h-6 items-center gap-1 rounded px-1.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
      >
        {copied ? <Check size={12} className="text-emerald-500" /> : <Copy size={12} />}
        {t("chat.message.copy")}
      </button>
      <button
        type="button"
        onClick={() => void fork()}
        title={t("chat.message.fork")}
        className="flex h-6 items-center gap-1 rounded px-1.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
      >
        <GitFork size={12} />
        {t("chat.message.fork")}
      </button>
    </div>
  );
}

function Markdown({ content }: { content: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      rehypePlugins={[rehypeHighlight]}
      components={{
        pre: ({ children }) => (
          <pre className="overflow-x-auto rounded-lg border bg-muted/60 p-3 text-xs">{children}</pre>
        ),
        a: ({ children, ...props }) => (
          <a {...props} target="_blank" rel="noreferrer" className="text-primary underline">
            {children}
          </a>
        ),
      }}
    >
      {content}
    </ReactMarkdown>
  );
}
