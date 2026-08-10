/**
 * 聊天窗口：标题栏（模型选择）、消息列表、输入区。
 */
import { useEffect, useMemo, useRef } from "react";
import { Square, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/stores/app-store";
import { MessageItem } from "./MessageItem";
import { ChatInput } from "./ChatInput";

/** 当前可用的模型选项：各提供商默认模型 -> 全部模型 */
function useModelOptions() {
  const config = useAppStore((s) => s.config);
  return useMemo(() => {
    if (!config) return [];
    return config.providers.flatMap((p) =>
      p.models.map((m) => ({
        value: `${p.id}::${m}`,
        label: m,
        provider: p.name,
      })),
    );
  }, [config]);
}

export function ChatWindow() {
  const messages = useAppStore((s) => s.messages);
  const streaming = useAppStore((s) => s.streaming);
  const streamingText = useAppStore((s) => s.streamingText);
  const stopStreaming = useAppStore((s) => s.stopStreaming);
  const config = useAppStore((s) => s.config);
  const activeSessionId = useAppStore((s) => s.activeSessionId);

  const modelOptions = useModelOptions();
  const scrollRef = useRef<HTMLDivElement>(null);

  // 新消息或流式输出时自动滚动到底部
  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages, streamingText, streaming]);

  const currentModel = useAppStore((s) => s.health?.version) ? (config?.default_model ?? "") : "";

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
          <Select value={currentModel || undefined}>
            <SelectTrigger className="h-7 w-44 text-xs">
              <SelectValue placeholder="选择模型" />
            </SelectTrigger>
            <SelectContent>
              {modelOptions.map((opt) => (
                <SelectItem key={opt.value} value={opt.value} className="text-xs">
                  <span className="flex items-center gap-1.5">
                    <Sparkles size={12} className="shrink-0 text-muted-foreground" />
                    {opt.label}
                    <span className="text-muted-foreground">· {opt.provider}</span>
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </header>

      {/* 消息列表 */}
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto">
        {messages.length === 0 && !streaming ? (
          <EmptyState />
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-5 px-4 py-6">
            {messages.map((m) => (
              <MessageItem key={m.id} message={m} />
            ))}
            {streaming && (
              <div className="flex gap-3">
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

      {/* 输入区 */}
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
