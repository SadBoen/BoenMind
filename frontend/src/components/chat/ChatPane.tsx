/**
 * 宿主共享对话面板（架构 §四·B 补充 v0.22，用户拍板"对话界面 = 宿主能力"）：
 * 消息流/输入/工具调用/流式渲染是协议层投影，任何应用要对话 = 嵌入本组件并
 * 绑定自己的会话（会话 `app` 场景字段），不需要则不嵌——聊天应用用 full
 * 形态，编程壳右栏用 panel 形态（Tab 切任务/对话）。
 *
 * 会话上下文 = store 的 activeSessionId（全局聚焦会话）：宿主切换应用时
 * 由 activateApp/ensureAppSession 把聚焦会话切到该场景，本组件零感知。
 */
import { useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Square, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { Message } from "@/api/client";
import { useAppStore } from "@/stores/app-store";
import { MessageItem } from "./MessageItem";
import { ChatInput } from "./ChatInput";
import { ScrollIndicators } from "./ScrollIndicators";
import { PermissionDialog } from "./PermissionDialog";
import { TaskStatusBar } from "./TaskStatusBar";
import { parseThinkBlocks } from "./ThinkBlock";

export interface ChatPaneProps {
  /** 形态变体：full = 全窗（聊天应用默认）；panel = 侧栏/Tab（紧凑标题栏 + 紧凑输入区） */
  variant?: "full" | "panel";
}

/** 消息预览文本（指示条悬停用）：剥离 think 块（含流式未闭合的）、压缩空白 */
function previewFor(message: Message, emptyLabel: string): string {
  const text = parseThinkBlocks(message.content)
    .filter((s) => s.type === "text")
    .map((s) => s.content)
    .join(" ")
    .replace(/\s+/g, " ")
    .trim();
  return text.length > 60 ? `${text.slice(0, 60)}…` : text || emptyLabel;
}

export function ChatPane({ variant = "full" }: ChatPaneProps) {
  const { t } = useTranslation();
  const panel = variant === "panel";
  const messages = useAppStore((s) => s.messages);
  const streaming = useAppStore((s) => s.streaming);
  const streamingText = useAppStore((s) => s.streamingText);
  const streamingToolCalls = useAppStore((s) => s.streamingToolCalls);
  const stopStreaming = useAppStore((s) => s.stopStreaming);
  const activeSessionId = useAppStore((s) => s.activeSessionId);

  // 预览文本按消息 id 缓存：流式期间 streamingText 每增量都重渲染本组件，
  // 不能对全部历史消息逐条跑正则（messages 引用在流式时不变化，memo 不重算）
  const previews = useMemo(() => {
    const map = new Map<number, string>();
    for (const m of messages) {
      map.set(m.id, previewFor(m, t("chat.previewEmpty")));
    }
    return map;
  }, [messages, t]);

  const scrollRef = useRef<HTMLDivElement>(null);
  // 避免流式增量导致的抖动：仅在用户未手动上翻时跟随
  const stickToBottom = useRef(true);

  // 新消息或流式输出时平滑滚动到底部（流式中快速跟随，新消息平滑动画）
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !stickToBottom.current) return;
    el.scrollTo({ top: el.scrollHeight, behavior: streaming ? "auto" : "smooth" });
  }, [messages, streamingText, streamingToolCalls, streaming]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
  };

  // overflow-hidden：防止 rail 横条 hover 变长时横向溢出到外层容器
  // （overflow-y:auto）触发滚动条，把整个窗口挤矮 15px 导致输入框跳动
  return (
    <div className="flex h-full min-w-0 flex-col overflow-hidden bg-background">
      {/* 标题栏：full = 会话标题/引导提示；panel = 仅会话标题（紧凑） */}
      <header
        className={cn(
          "flex shrink-0 items-center justify-between border-b px-3",
          panel ? "h-9" : "h-11",
        )}
      >
        <div className="flex min-w-0 items-center gap-2 text-sm font-medium">
          {activeSessionId ? (
            <span className="truncate">
              {messages[0]?.role === "user" ? previewFor(messages[0], t("chat.newSession")) : t("chat.newSession")}
            </span>
          ) : (
            <span className="text-muted-foreground">
              {panel ? t("chat.panelTitle") : t("chat.selectOrCreate")}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {streaming && (
            <Button size="sm" variant="outline" className="h-7 gap-1 text-xs" onClick={stopStreaming}>
              <Square size={12} className="fill-current" />
              {t("chat.stop")}
            </Button>
          )}
        </div>
      </header>

      {/* 任务状态条：活跃心跳 + 最近任务终态/断线续跑恢复 */}
      <TaskStatusBar />

      {/* 消息列表：隐藏滚动条 + 内容平滑增长；指示条放在滚动容器外做固定 overlay，
          否则 absolute 定位会随内容滚动，滚到底部时 rail 被带出视口 */}
      <div className="relative min-h-0 flex-1">
        <div
          ref={scrollRef}
          onScroll={onScroll}
          className="h-full overflow-y-auto scrollbar-none"
        >
          {messages.length === 0 && !streaming ? (
            <EmptyState />
          ) : (
            <div className="flex flex-col gap-5 px-6 py-6">
              {messages.map((m, i) => (
                <div key={m.id} data-mid={i} data-role={m.role} data-preview={previews.get(m.id)}>
                  <MessageItem message={m} />
                </div>
              ))}
              {streaming && (
                <div
                  className={cn(
                    "msg-enter",
                    !streamingText && streamingToolCalls.length === 0 && "opacity-60",
                  )}
                >
                  {streamingText || streamingToolCalls.length > 0 ? (
                    <MessageItem
                      message={{
                        id: -1,
                        session_id: "",
                        role: "assistant",
                        content: streamingText,
                        created_at: 0,
                      }}
                      streaming
                      streamingToolCalls={streamingToolCalls.map((c) => ({
                        tool_name: c.name,
                        args: c.args,
                        is_error: c.isError,
                        // 收到 ToolCallEnd 前保持"执行中"中性色
                        running: !c.done,
                      }))}
                    />
                  ) : (
                    <div className="flex items-center gap-1.5 py-2">
                      <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-muted-foreground/50" />
                      <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-muted-foreground/50 [animation-delay:120ms]" />
                      <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-muted-foreground/50 [animation-delay:240ms]" />
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
        <ScrollIndicators containerRef={scrollRef} />
      </div>

      {/* 输入区（模型/思考选择在框内下边缘；panel 形态隐藏提示与占位按钮） */}
      <ChatInput compact={panel} />
      {/* 插件权限询问弹窗（SSE permissionRequest 事件触发） */}
      <PermissionDialog />
    </div>
  );
}

function EmptyState() {
  const { t } = useTranslation();
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
      <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-primary/10 text-primary">
        <Sparkles size={28} />
      </div>
      <div>
        <h3 className="text-base font-semibold">{t("chat.emptyTitle")}</h3>
        <p className="mt-1 max-w-xs text-sm text-muted-foreground">
          {t("chat.emptyHint")}
        </p>
      </div>
    </div>
  );
}
