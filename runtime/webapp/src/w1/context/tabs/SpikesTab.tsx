//! TAB 5: 多轮 Token 暴增与刺客诊断(#22 拆分:自 context.tsx 机械移入)
//! 对比相邻快照的输入增量,标记因外部搜索/大文件读入导致的上下文爆仓。

import { TrendingUp, AlertTriangle } from "lucide-react";
import { cn } from "@/lib/utils";
import type { SpikeItem } from "../types";

export function SpikesTab({ items }: { items: SpikeItem[] }) {
  return (
    <div className="flex flex-col gap-3 rounded-xl border bg-card p-3.5 shadow-2xs">
      <div className="flex flex-wrap items-center justify-between border-b pb-2 text-[12.5px]">
        <div>
          <span className="font-semibold text-foreground flex items-center gap-1.5">
            <TrendingUp className="size-4 text-primary" />
            <span>多轮对话 Token 暴增与刺客诊断 (Spike Alert)</span>
          </span>
          <p className="text-[11.5px] text-muted-foreground mt-0.5">
            自动对比相邻轮次的 Token 增量，智能揪出是哪一轮因外部搜索、读入超大文件导致上下文突然爆仓。
          </p>
        </div>
      </div>

      <div className="flex flex-col gap-2">
        {items.map((item, _idx) => (
          <div
            key={item.seq}
            className={cn(
              "flex items-center justify-between rounded-lg border p-3 text-[12px] transition-colors",
              item.isSpike ? "border-rose-500/50 bg-rose-500/10" : "bg-muted/20 border-border/60",
            )}
          >
            <div className="flex items-center gap-3">
              <span className="font-mono font-semibold text-[13px] text-foreground">
                第 {item.turn_index} 轮 (第 {item.step} 步)
              </span>
              <span className="text-muted-foreground text-[11.5px]">
                输入: <strong className="text-foreground">{item.tokens_in} token</strong> · 输出: {item.tokens_out} token
              </span>
              {item.diff > 0 ? (
                <span className={cn(
                  "rounded-md px-1.5 py-0.5 text-[11px] font-medium font-mono",
                  item.isSpike ? "bg-rose-500 text-white" : "bg-muted text-muted-foreground"
                )}>
                  +{item.diff} token
                </span>
              ) : null}
            </div>

            <div className="flex items-center gap-2">
              {item.isSpike ? (
                <span className="flex items-center gap-1 rounded bg-rose-500/20 px-2 py-0.5 text-[11.5px] font-semibold text-rose-600 dark:text-rose-400">
                  <AlertTriangle className="size-3.5" />
                  <span>⚠️ 检测到 Token 异常激增！可能调用了携带大量长文的外部工具</span>
                </span>
              ) : (
                <span className="text-[11.5px] text-muted-foreground">正常增长</span>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
