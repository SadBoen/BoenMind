/**
 * 消息渲染：用户气泡 + 助手（思考/工具执行过程折叠块 + 正式答复 Markdown）。
 * 对齐 pi-web 语义：绿条（工具调用）属于"过程"，收纳在思考/执行折叠块内，
 * 正式答复（纯文本）下方不再显示工具块。
 */
import { memo, useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { Bot, User } from "lucide-react";
import { cn } from "@/lib/utils";
import type { Message } from "@/api/client";
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
    <div className="msg-enter flex gap-3">
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
      </div>
    </div>
  );
});

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
