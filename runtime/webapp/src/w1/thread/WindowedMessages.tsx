// #27 长会话窗口化渲染(#22 拆分:自 thread.tsx 机械移入):
// DOM 只挂最近 history.windowSize 条消息(大代码块/密集表格是掉帧主源),
// 更早已加载部分经「展开更早」即时入窗不取数;窗口盖满已加载且服务端还有
// 更早时才触发 loadOlder 取数。消息经 Unstable_MessageById(id 锚定,窗口化/
// 重排不脱钩)渲染,assistant-ui 消息上下文链与 UserMessage/AssistantMessage
// 组件零改动。
import { ThreadPrimitive, unstable_useThreadMessageIds } from "@assistant-ui/react";
import { useBoenmindApprovals } from "../runtime";
import { UserMessage } from "./UserMessage";
import { AssistantMessage } from "./AssistantMessage";

const MSG_WINDOW_STEP = 200;

export function WindowedMessages() {
  const { history } = useBoenmindApprovals();
  const ids = unstable_useThreadMessageIds();
  const hidden = Math.max(0, ids.length - history.windowSize);
  const visible = hidden > 0 ? ids.slice(hidden) : ids;
  const topCtl =
    hidden > 0 ? (
      <div className="flex justify-center py-2">
        <button
          onClick={history.expandOlder}
          data-slot="expand-older"
          className="rounded-md border border-border/50 bg-background/70 px-3 py-1 text-[12px] text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground"
        >
          展开更早 {Math.min(hidden, MSG_WINDOW_STEP)} 条(已加载,即时显示)
        </button>
      </div>
    ) : history.capped ? (
      <div className="flex justify-center py-2 text-[12px] text-muted-foreground">
        已达本会话累计加载上限,更早历史不再载入
      </div>
    ) : history.hasMore ? (
      <div className="flex justify-center py-2">
        <button
          onClick={history.loadOlder}
          disabled={history.loading}
          data-slot="load-older"
          className="rounded-md border border-border/50 bg-background/70 px-3 py-1 text-[12px] text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground disabled:opacity-60"
        >
          {history.loading ? "加载中…" : "加载更早消息"}
        </button>
      </div>
    ) : null;
  return (
    <>
      {topCtl}
      {visible.map((id) => (
        <ThreadPrimitive.Unstable_MessageById
          key={id}
          messageId={id}
          components={{ UserMessage, AssistantMessage }}
        />
      ))}
    </>
  );
}
