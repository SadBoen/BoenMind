/**
 * 消息渲染：用户气泡 + 助手 Markdown（含代码高亮 + <think> 折叠块 + 工具调用块）。
 */
import { memo, useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { Bot, User } from "lucide-react";
import { cn } from "@/lib/utils";
import type { Message } from "@/api/client";
import { ThinkBlock, hasThinkBlock, parseThinkBlocks } from "./ThinkBlock";
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

  // 助手消息：解析 think 块（有思考内容时按分段渲染，否则整条走 markdown）
  const segments = useMemo(() => {
    if (isUser) return null;
    return hasThinkBlock(message.content) ? parseThinkBlocks(message.content) : null;
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

  return (
    <div className="msg-enter flex gap-3">
      <div className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
        <Bot size={14} />
      </div>
      <div className="min-w-0 flex-1">
        {toolCalls.length > 0 && (
          <div className="mb-2 flex flex-col gap-1.5">
            {toolCalls.map((c, i) => (
              <ToolCallBlock key={i} call={c} />
            ))}
          </div>
        )}
        <div className={cn("prose prose-sm dark:prose-invert max-w-none break-words", streaming && "animate-pulse")}>
          {segments ? (
            <SegmentedMarkdown segments={segments} />
          ) : (
            <Markdown content={message.content} />
          )}
        </div>
      </div>
    </div>
  );
});

/** 分段渲染：text 段走 markdown，think 段走折叠块 */
function SegmentedMarkdown({ segments }: { segments: ReturnType<typeof parseThinkBlocks> }) {
  return (
    <>
      {segments.map((seg, i) =>
        seg.type === "think" ? (
          <ThinkBlock key={i} content={seg.content} open={seg.open} />
        ) : (
          <Markdown key={i} content={seg.content} />
        ),
      )}
    </>
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
