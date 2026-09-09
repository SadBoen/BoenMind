//! TAB 3: 聊天记忆双栏联动 + 对话遗忘健康度(#22 拆分:自 context.tsx 机械移入)
//! 左侧对答卡片 + 遗忘说明卡(台账 20 轮/24K 字符双上限),右侧历史报文块。

import { Code2, Copy, Check, Scissors } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { estTokens, scrollToId, type ParsedPromptRecipe } from "../utils";
import type { ContextStats } from "../types";

export function MemoryTab({
  recipe,
  stats,
  selectedTurnIndex,
  onSelectTurn,
  copiedKey,
  onCopy,
}: {
  recipe: ParsedPromptRecipe;
  stats: ContextStats | null;
  selectedTurnIndex: number | null;
  onSelectTurn: (idx: number) => void;
  copiedKey: string | null;
  onCopy: (key: string, text: string) => void;
}) {
  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex flex-wrap items-center justify-between border-b pb-2 text-[12.5px]">
        <div>
          <span className="font-semibold text-foreground">💬 依然清晰保留的聊天记忆 (双栏联动透视)</span>
          <span className="ml-2 text-[11.5px] text-muted-foreground">
            点击左侧对答卡片，右侧历史消息报文自动滚动并加深高亮
          </span>
        </div>
        <span className="rounded-md bg-sky-500/10 px-2 py-0.5 text-[11.5px] font-medium text-sky-600 dark:text-sky-400">
          当前存活 {recipe.historyTurns.length} 轮 (上限 20 轮)
        </span>
      </div>

      <div className="grid grid-cols-1 gap-3.5 lg:grid-cols-12 min-h-[440px]">
        {/* 左侧：对答卡片 */}
        <div className="flex flex-col gap-2.5 overflow-y-auto pr-1 lg:col-span-5 max-h-[500px]">
          {recipe.historyTurns.length === 0 ? (
            <div className="rounded-xl border bg-card p-6 text-center text-[12.5px] text-muted-foreground">
              这是新会话的第一轮对话，暂无前期聊天记忆
            </div>
          ) : (
            recipe.historyTurns.map((h) => {
              const isSelected = selectedTurnIndex === h.turnIndex;
              const turnTokens = estTokens(h.user) + estTokens(h.assistant);
              return (
                <div
                  key={h.turnIndex}
                  onClick={() => {
                    onSelectTurn(h.turnIndex);
                    scrollToId(`history-turn-${h.turnIndex}`);
                  }}
                  className={cn(
                    "cursor-pointer rounded-lg border p-3 transition-all duration-150 flex flex-col justify-between gap-1.5",
                    isSelected
                      ? "border-primary bg-primary/10 shadow-xs ring-1 ring-primary/40"
                      : "bg-card hover:border-border hover:bg-muted/30 border-border/70",
                  )}
                >
                  <div className="flex items-center justify-between text-[11.5px]">
                    <span className="font-semibold text-foreground">第 {h.turnIndex} 轮对答记忆</span>
                    <span className="text-muted-foreground text-[10.5px]">约 {turnTokens} token</span>
                  </div>
                  <div className="text-[12px] font-medium text-foreground/90 line-clamp-2">
                    问: {h.user}
                  </div>
                  <div className="text-[11.5px] text-muted-foreground line-clamp-3">
                    答: {h.assistant}
                  </div>
                  <div className="flex items-center justify-between border-t border-border/40 pt-1.5 text-[11px]">
                    <span className="text-muted-foreground">问 {estTokens(h.user)} · 答 {estTokens(h.assistant)} token</span>
                    <span className={cn("font-medium", isSelected ? "text-primary" : "text-muted-foreground/60")}>
                      {isSelected ? "✓ 正在右侧高亮" : "点击查看报文"}
                    </span>
                  </div>
                </div>
              );
            })
          )}

          {(stats?.evictedTurns ?? 0) > 0 ? (
            <div className="rounded-xl border border-amber-500/40 p-3 text-[11.5px] bg-amber-500/10 flex items-start gap-2.5">
              <Scissors className="size-4 mt-0.5 text-amber-600 dark:text-amber-400 shrink-0" />
              <div>
                <span className="font-semibold text-amber-600 dark:text-amber-400">已经有对话被自动遗忘：</span>
                <span className="text-muted-foreground">
                  台账上限为 20 轮或 24,000 字符，最早的 <strong className="text-foreground">{stats?.evictedTurns}</strong> 轮已从 AI 的记忆里裁掉。上面的卡片是它现在还真正记得的全部内容。
                </span>
              </div>
            </div>
          ) : (
            <div className="rounded-xl border border-dashed p-3 text-[11.5px] text-muted-foreground bg-muted/10 flex items-start gap-2.5">
              <Scissors className="size-4 mt-0.5 text-muted-foreground shrink-0" />
              <div>
                <span className="font-semibold text-foreground">关于对话遗忘的说明：</span>
                <span>
                  系统上限为 20 轮或 24,000 字符。截至最近一次调用，本对话的所有历史轮次都还在，没有被剪掉。
                </span>
              </div>
            </div>
          )}
        </div>

        {/* 右侧：实际回喂给模型的历史报文块 */}
        <div className="flex flex-col gap-2.5 overflow-y-auto rounded-xl border bg-muted/20 p-3 lg:col-span-7 max-h-[500px]">
          <div className="flex items-center justify-between border-b border-border/60 pb-1.5 text-[12px]">
            <span className="font-semibold text-foreground flex items-center gap-1.5">
              <Code2 className="size-3.5 text-primary" />
              <span>历史消息原始报文 (OpenAI Messages 格式)</span>
            </span>
            <span className="text-[11px] font-mono text-muted-foreground">
              {recipe.historyTurns.length * 2} messages
            </span>
          </div>

          <div className="flex flex-col gap-3">
            {recipe.historyTurns.length === 0 ? (
              <div className="p-8 text-center text-[12px] text-muted-foreground">
                无历史报文
              </div>
            ) : (
              recipe.historyTurns.map((h) => {
                const isSelected = selectedTurnIndex === h.turnIndex;
                const turnJson = [
                  { role: "user", content: h.user },
                  { role: "assistant", content: h.assistant },
                ];
                return (
                  <div
                    key={h.turnIndex}
                    id={`history-turn-${h.turnIndex}`}
                    className={cn(
                      "rounded-lg border p-2.5 transition-all duration-200",
                      isSelected
                        ? "border-primary bg-primary/10 shadow-sm ring-1 ring-primary/30"
                        : "border-border/60 bg-background/70 hover:border-border",
                    )}
                  >
                    <div className="mb-1.5 flex items-center justify-between text-[11.5px]">
                      <div className="flex items-center gap-1.5 font-mono font-medium">
                        <span className={cn("size-2 rounded-full", isSelected ? "bg-primary" : "bg-muted-foreground")} />
                        <span className="text-foreground">第 {h.turnIndex} 轮对答报文</span>
                      </div>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-6 gap-1 px-1.5 text-[10.5px] text-muted-foreground hover:text-foreground"
                        onClick={() => onCopy(`turn_${h.turnIndex}`, JSON.stringify(turnJson, null, 2))}
                      >
                        {copiedKey === `turn_${h.turnIndex}` ? (
                          <>
                            <Check className="size-3 text-emerald-500" />
                            <span className="text-emerald-500">已复制</span>
                          </>
                        ) : (
                          <>
                            <Copy className="size-3" />
                            <span>复制此轮</span>
                          </>
                        )}
                      </Button>
                    </div>

                    <pre className="max-h-48 overflow-auto rounded bg-muted/40 p-2 font-mono text-[11px] leading-relaxed text-foreground/90 whitespace-pre-wrap break-all">
                      {JSON.stringify(turnJson, null, 2)}
                    </pre>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
