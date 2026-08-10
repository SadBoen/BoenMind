/**
 * 消息渲染：用户气泡 + 助手 Markdown（含代码高亮）。
 */
import { memo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { Bot, User } from "lucide-react";
import { cn } from "@/lib/utils";
import type { Message } from "@/api/client";

export const MessageItem = memo(function MessageItem({
  message,
  streaming,
}: {
  message: Message;
  streaming?: boolean;
}) {
  const isUser = message.role === "user";

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
        <div
          className={cn(
            "prose prose-sm dark:prose-invert max-w-none break-words",
            streaming && "animate-pulse",
          )}
        >
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            rehypePlugins={[rehypeHighlight]}
            components={{
              pre: ({ children }) => (
                <pre className="overflow-x-auto rounded-lg border bg-muted/60 p-3 text-xs">
                  {children}
                </pre>
              ),
              a: ({ children, ...props }) => (
                <a {...props} target="_blank" rel="noreferrer" className="text-primary underline">
                  {children}
                </a>
              ),
            }}
          >
            {message.content}
          </ReactMarkdown>
        </div>
      </div>
    </div>
  );
});
