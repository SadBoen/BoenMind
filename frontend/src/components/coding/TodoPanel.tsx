/**
 * 活任务清单面板（M2 核心，用户痛点"任务清单生成后不会实时插入/删除"）：
 * - 数据源 = 会话事件流投影（subscribeEvents after=0 重放 + 实时），
 *   todo/write 是全量快照事件，重放即得初始状态，无需单独拉取；
 * - 手动操作（添加/切换状态/删除）→ REST 面（同一事件链，模型与用户
 *   的改动一视同仁，前端投影对两者实时刷新）；
 * - 回合步数投影（M2 面板：step/start 事件计数，max_steps 预算由引擎
 *   侧"步数预算提示"负责收敛，前端只展示进度）。
 */
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, ListTodo, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import { api, type SessionEvent } from "@/api/client";
import { useAppStore } from "@/stores/app-store";

/** 状态环顺序：点击循环推进（pending → in_progress → completed） */
const STATUS_CYCLE = ["pending", "in_progress", "completed"] as const;
type TodoStatus = (typeof STATUS_CYCLE)[number];

export function TodoPanel() {
  const { t } = useTranslation();
  const activeSessionId = useAppStore((s) => s.activeSessionId);
  const todos = useAppStore((s) => s.todos);
  const setTodosFromEvent = useAppStore((s) => s.setTodosFromEvent);
  const applyTodoOp = useAppStore((s) => s.applyTodoOp);
  const [draft, setDraft] = useState("");
  const [connected, setConnected] = useState(false);
  const [steps, setSteps] = useState(0);
  // 首个活动项引用（自动滚动：清单长起来后不拉滚动条也能看到当前工作）
  const firstActiveRef = useRef<HTMLDivElement | null>(null);

  // 事件流订阅：todo/write 投影 + step/start 计数（同一通道）
  useEffect(() => {
    if (!activeSessionId) return;
    setSteps(0);
    const close = api.subscribeEvents(activeSessionId, (ev: SessionEvent) => {
      if (ev.type === "todo/write" && Array.isArray(ev.todos)) {
        setTodosFromEvent(ev.todos);
      } else if (ev.type === "step/start" && typeof ev.step === "number") {
        setSteps(ev.step);
      }
    });
    setConnected(true);
    return () => {
      close();
      setConnected(false);
    };
  }, [activeSessionId, setTodosFromEvent]);

  // 首个未完成任务索引（自动滚动目标 + ref 挂载点）
  const firstActiveIdx = todos.findIndex((t) => t.status !== "completed");

  // 自动滚动到首个未完成任务（"清单动起来"）：清单更新后若活动项不在
  // 视口内，就近滚入——完成项堆在顶部时无需手动拉滚动条。block:"nearest"
  // 只在元素不可见时滚动，用户主动翻历史时不被抢视口。
  useEffect(() => {
    if (firstActiveIdx >= 0 && firstActiveRef.current) {
      firstActiveRef.current.scrollIntoView({ block: "nearest" });
    }
  }, [todos, firstActiveIdx]);

  const addTodo = async () => {
    const content = draft.trim();
    if (!content) return;
    setDraft("");
    await applyTodoOp({ action: "add", content });
  };

  const cycleStatus = async (index: number) => {
    const item = todos[index];
    if (!item) return;
    const next =
      STATUS_CYCLE[(STATUS_CYCLE.indexOf(item.status as TodoStatus) + 1) % STATUS_CYCLE.length];
    await applyTodoOp({ action: "update", index: index + 1, status: next });
  };

  const removeTodo = async (index: number) => {
    await applyTodoOp({ action: "remove", index: index + 1 });
  };

  if (!activeSessionId) {
    return (
      <PanelFrame title={t("coding.todos.title")}>
        <p className="px-3 py-6 text-center text-xs text-muted-foreground">
          {t("coding.todos.noSession")}
        </p>
      </PanelFrame>
    );
  }

  return (
    <PanelFrame title={t("coding.todos.title")}>
      {/* 订阅状态 + 回合步数（M2 面板） */}
      <div className="flex shrink-0 items-center gap-2 border-b px-3 py-1.5 text-[10px] text-muted-foreground">
        <span
          className={cn(
            "inline-flex items-center gap-1",
            connected ? "text-green-600" : "text-muted-foreground",
          )}
        >
          <span className={cn("h-1.5 w-1.5 rounded-full", connected ? "bg-green-500" : "bg-muted-foreground/40")} />
          {connected ? t("coding.todos.live") : t("coding.todos.offline")}
        </span>
        {steps > 0 && (
          <span className="ml-auto inline-flex items-center gap-1">
            <Loader2 size={10} className="animate-spin" />
            {t("coding.todos.steps", { count: steps })}
          </span>
        )}
      </div>

      {/* 清单 */}
      <ScrollArea className="min-h-0 flex-1">
        <div className="px-2 py-2">
          {todos.length === 0 && (
            <p className="py-8 text-center text-xs text-muted-foreground">
              {t("coding.todos.empty")}
            </p>
          )}
          {todos.map((item, i) => (
            <div
              key={`${i}-${item.content}`}
              ref={i === firstActiveIdx ? firstActiveRef : undefined}
              className="group flex items-start gap-2 rounded-md px-2 py-1.5 hover:bg-accent/50"
            >
              <button
                type="button"
                onClick={() => void cycleStatus(i)}
                title={t("coding.todos.cycleStatus")}
                className={cn(
                  "mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full border text-[9px] transition-colors",
                  item.status === "completed"
                    ? "border-green-500 bg-green-500 text-white"
                    : item.status === "in_progress"
                      ? "border-amber-400 text-amber-500"
                      : "border-muted-foreground/40 text-transparent",
                )}
              >
                {item.status === "completed" ? "✓" : item.status === "in_progress" ? "◐" : "○"}
              </button>
              <div className="min-w-0 flex-1">
                <p
                  className={cn(
                    "break-words text-xs leading-relaxed",
                    item.status === "completed" && "text-muted-foreground line-through",
                  )}
                >
                  {item.content}
                </p>
                {item.priority && (
                  <span className="mt-0.5 inline-block rounded bg-muted px-1 text-[9px] text-muted-foreground">
                    {item.priority}
                  </span>
                )}
              </div>
              <button
                type="button"
                onClick={() => void removeTodo(i)}
                className="shrink-0 rounded p-0.5 text-muted-foreground/50 opacity-0 transition-opacity hover:text-destructive group-hover:opacity-100"
                title={t("common.delete")}
              >
                <Trash2 size={12} />
              </button>
            </div>
          ))}
        </div>
      </ScrollArea>

      {/* 添加 */}
      <div className="flex shrink-0 items-center gap-1.5 border-t p-2">
        <Input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void addTodo();
          }}
          placeholder={t("coding.todos.addPlaceholder")}
          className="h-7 text-xs"
        />
        <Button
          size="icon"
          className="h-7 w-7 shrink-0"
          disabled={!draft.trim()}
          onClick={() => void addTodo()}
          title={t("coding.todos.add")}
        >
          <Plus size={14} />
        </Button>
      </div>
    </PanelFrame>
  );
}

function PanelFrame({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex h-full min-w-0 flex-col bg-background">
      <div className="flex h-11 shrink-0 items-center gap-2 border-b px-3">
        <ListTodo size={14} className="text-primary" />
        <span className="text-sm font-medium">{title}</span>
      </div>
      <div className="flex min-h-0 flex-1 flex-col">{children}</div>
    </div>
  );
}
