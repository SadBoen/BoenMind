/**
 * 聊天窗口：标题栏（停止按钮）、消息列表（平滑滚动 + 淡入动画）、输入区。
 * 模型与思考强度选择已移至输入框内部下边缘（见 ChatInput）。
 */
import { useEffect, useRef } from "react";
import { Square, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/stores/app-store";
import { MessageItem } from "./MessageItem";
import { ChatInput } from "./ChatInput";

export function ChatWindow() {
  const messages = useAppStore((s) => s.messages);
  const streaming = useAppStore((s) => s.streaming);
  const streamingText = useAppStore((s) => s.streamingText);
  const stopStreaming = useAppStore((s) => s.stopStreaming);
  const activeSessionId = useAppStore((s) => s.activeSessionId);

  const scrollRef = useRef<HTMLDivElement>(null);
  // 避免流式增量导致的抖动：仅在用户未手动上翻时跟随
  const stickToBottom = useRef(true);

  // 新消息或流式输出时平滑滚动到底部（流式中快速跟随，新消息平滑动画）
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !stickToBottom.current) return;
    el.scrollTo({ top: el.scrollHeight, behavior: streaming ? "auto" : "smooth" });
  }, [messages, streamingText, streaming]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
  };

  return (
    <div className="flex h-full min-w-0 flex-col bg-background">
      {/* 标题栏 */}
      <header className="flex h-11 shrink-0 items-center justify-between border-b px-3">
        <div className="flex min-w-0 items-center gap-2 text-sm font-medium">
          {activeSessionId ? (
            <span className="truncate">
              {messages[0]?.role === "user" ? messages[0].content.slice(0, 24) : "新对话"}
            </span>
          ) : (
            <span className="text-muted-foreground">选择或新建一个对话</span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {streaming && (
            <Button size="sm" variant="outline" className="h-7 gap-1 text-xs" onClick={stopStreaming}>
              <Square size={12} className="fill-current" />
              停止
            </Button>
          )}
        </div>
      </header>

      {/* 消息列表：隐藏滚动条 + 内容平滑增长 */}
      <div
        ref={scrollRef}
        onScroll={onScroll}
        className="min-h-0 flex-1 overflow-y-auto scrollbar-none"
      >
        {messages.length === 0 && !streaming ? (
          <EmptyState />
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-5 px-4 py-6">
            {messages.map((m) => (
              <MessageItem key={m.id} message={m} />
            ))}
            {streaming && (
              <div className="msg-enter flex gap-3">
                <div className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
                  <Sparkles size={14} />
                </div>
                <div className={cn("min-w-0 flex-1", !streamingText && "opacity-60")}>
                  {streamingText ? (
                    <MessageItem
                      message={{
                        id: -1,
                        session_id: "",
                        role: "assistant",
                        content: streamingText,
                        created_at: 0,
                      }}
                      streaming
                    />
                  ) : (
                    <div className="flex items-center gap-1.5 py-2">
                      <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-muted-foreground/50" />
                      <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-muted-foreground/50 [animation-delay:120ms]" />
                      <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-muted-foreground/50 [animation-delay:240ms]" />
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {/* 输入区（模型/思考选择在框内下边缘） */}
      <ChatInput />
    </div>
  );
}

function EmptyState() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
      <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-primary/10 text-primary">
        <Sparkles size={28} />
      </div>
      <div>
        <h3 className="text-base font-semibold">BoenMind 对话</h3>
        <p className="mt-1 max-w-xs text-sm text-muted-foreground">
          问我任何问题，或让我帮你处理工作文件夹中的任务。
        </p>
      </div>
    </div>
  );
}
