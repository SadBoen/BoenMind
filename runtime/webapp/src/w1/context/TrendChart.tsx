//! 【DSH 视觉化吸收:时序堆叠演进趋势图 + 单步三行白话速报】
//! (#22 拆分:自 context.tsx 视图块机械移入)
//! 全量/增量两种模式 + 按单步/按轮次聚合切换;点击柱子 = 时间旅行选中该步。

import { TrendingUp, User, ArrowDownLeft, ArrowUpRight, Scissors } from "lucide-react";
import { cn } from "@/lib/utils";
import type { TrendItem, DeltaTrendItem } from "./types";

export function TrendChart({
  trendItems,
  deltaTrendItems,
  maxTrendTokens,
  maxDeltaMagnitude,
  trendMode,
  onTrendModeChange,
  trendGranularity,
  onTrendGranularityChange,
  activeTrendStep,
  onSelectStep,
  hoveredCategory,
}: {
  trendItems: TrendItem[];
  deltaTrendItems: DeltaTrendItem[];
  maxTrendTokens: number;
  maxDeltaMagnitude: number;
  trendMode: "total" | "delta";
  onTrendModeChange: (m: "total" | "delta") => void;
  trendGranularity: "step" | "turn";
  onTrendGranularityChange: (g: "step" | "turn") => void;
  activeTrendStep: TrendItem | null;
  onSelectStep: (id: string) => void;
  hoveredCategory: string | null;
}) {
  return (
    <div className="bg-card rounded-xl border p-3.5 shadow-2xs flex flex-col gap-3">
      <div className="flex items-center justify-between border-b pb-2">
        <div className="flex items-center gap-2">
          <TrendingUp className="size-4 text-primary" />
          <span className="text-[13px] font-semibold text-foreground">
            上下文演进趋势图 (时序堆叠)
          </span>
          <span className="text-[11.5px] text-muted-foreground">
            记录每一次调用时上下文体积的演进与构成膨胀
          </span>
        </div>

        <div className="flex items-center gap-2">
          {/* 总量/增量模式切换 (阶段三:Delta 模式) */}
          <div className="flex items-center gap-1 bg-muted/60 p-0.5 rounded-lg border">
            <button
              type="button"
              onClick={() => onTrendModeChange("total")}
              className={cn(
                "rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors",
                trendMode === "total" ? "bg-background shadow-xs text-foreground" : "text-muted-foreground hover:text-foreground"
              )}
              title="全量模式:柱高 = 该步上下文总体积"
            >
              全量 (Total)
            </button>
            <button
              type="button"
              onClick={() => onTrendModeChange("delta")}
              className={cn(
                "rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors",
                trendMode === "delta" ? "bg-background shadow-xs text-foreground" : "text-muted-foreground hover:text-foreground"
              )}
              title="增量模式:相对上一步的 Token 净变化;向下突破基准 = 压缩/裁剪释放"
            >
              增量 (Delta)
            </button>
          </div>

          <div className="flex items-center gap-1 bg-muted/60 p-0.5 rounded-lg border">
            <button
              type="button"
              onClick={() => onTrendGranularityChange("step")}
              className={cn(
                "rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors",
                trendGranularity === "step" ? "bg-background shadow-xs text-foreground" : "text-muted-foreground hover:text-foreground"
              )}
            >
              按单步 (Step)
            </button>
            <button
              type="button"
              onClick={() => onTrendGranularityChange("turn")}
              className={cn(
                "rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors",
                trendGranularity === "turn" ? "bg-background shadow-xs text-foreground" : "text-muted-foreground hover:text-foreground"
              )}
            >
              按轮次 (Turn)
            </button>
          </div>
        </div>
      </div>

      {/* 柱状演进图 */}
      <div className={cn(
        "flex gap-2.5 overflow-x-auto px-2",
        trendMode === "total" ? "items-end pb-1 pt-4 min-h-[140px]" : "items-center min-h-[160px]"
      )}>
        {trendMode === "total"
          ? trendItems.map((item) => {
          const isSelected = activeTrendStep?.id === item.id;
          const heightPct = Math.max(15, Math.min(100, Math.round(((item.tokens_in + item.tokens_out) / maxTrendTokens) * 100)));

          // 6 色高度细分
          const pPct = (item.pTok / item.total) * 100;
          const skPct = (item.skTok / item.total) * 100;
          const wsPct = (item.wsTok / item.total) * 100;
          const toolPct = (item.toolTok / item.total) * 100;
          const histPct = (item.histTok / item.total) * 100;
          const inPct = (item.inTok / item.total) * 100;

          return (
            <div
              key={item.id}
              onClick={() => onSelectStep(item.id)}
              className="group flex flex-col items-center gap-1.5 cursor-pointer shrink-0"
              style={{ width: trendItems.length <= 8 ? "64px" : "48px" }}
            >
              {/* 柱顶：剪枝标记 / Token 标尺 */}
              <div className="flex flex-col items-center gap-0.5">
                {item.evicted_turns > 0 ? (
                  <span className="text-amber-500" title={`此步已有 ${item.evicted_turns} 轮旧对话被裁剪`}>
                    <Scissors className="size-3" />
                  </span>
                ) : null}
                <span className="text-[10px] font-mono text-muted-foreground group-hover:text-foreground">
                  {item.tokens_in}
                </span>
              </div>

              {/* 堆叠色彩柱 */}
              <div
                style={{ height: `${heightPct}px` }}
                className={cn(
                  "w-full rounded-md overflow-hidden flex flex-col-reverse transition-all duration-200 border",
                  isSelected
                    ? "ring-2 ring-primary border-primary shadow-xs"
                    : "border-border/80 group-hover:border-primary/50 group-hover:shadow-xs"
                )}
              >
                {/* 人设 */}
                <div
                  style={{ height: `${pPct}%` }}
                  className={cn("bg-indigo-500 transition-opacity", hoveredCategory && hoveredCategory !== "persona" ? "opacity-25" : "opacity-100")}
                  title={`人设: 约 ${item.pTok} token`}
                />
                {/* 技能 */}
                {skPct > 0 ? (
                  <div
                    style={{ height: `${skPct}%` }}
                    className={cn("bg-purple-500 transition-opacity", hoveredCategory && hoveredCategory !== "skills" ? "opacity-25" : "opacity-100")}
                    title={`技能: 约 ${item.skTok} token`}
                  />
                ) : null}
                {/* 工具 */}
                <div
                  style={{ height: `${toolPct}%` }}
                  className={cn("bg-amber-500 transition-opacity", hoveredCategory && hoveredCategory !== "tools" ? "opacity-25" : "opacity-100")}
                  title={`工具: 约 ${item.toolTok} token`}
                />
                {/* 记忆 */}
                {histPct > 0 ? (
                  <div
                    style={{ height: `${histPct}%` }}
                    className={cn("bg-sky-500 transition-opacity", hoveredCategory && hoveredCategory !== "history" ? "opacity-25" : "opacity-100")}
                    title={`记忆: 约 ${item.histTok} token`}
                  />
                ) : null}
                {/* 目录 */}
                {wsPct > 0 ? (
                  <div
                    style={{ height: `${wsPct}%` }}
                    className={cn("bg-emerald-500 transition-opacity", hoveredCategory && hoveredCategory !== "ws" ? "opacity-25" : "opacity-100")}
                    title={`目录: 约 ${item.wsTok} token`}
                  />
                ) : null}
                {/* 问题 */}
                <div
                  style={{ height: `${inPct}%` }}
                  className={cn("bg-rose-500 transition-opacity", hoveredCategory && hoveredCategory !== "input" ? "opacity-25" : "opacity-100")}
                  title={`输入: 约 ${item.inTok} token`}
                />
              </div>

              {/* 柱底标签 */}
              <span className={cn(
                "text-[10.5px] font-mono",
                isSelected ? "font-bold text-primary" : "text-muted-foreground"
              )}>
                {item.label}
              </span>
            </div>
          );
        })
          : deltaTrendItems.map((d) => {
              const isSelected = activeTrendStep?.id === d.id;
              const heightPct = Math.max(6, Math.round((d.magnitude / maxDeltaMagnitude) * 100));

              return (
                <div
                  key={d.id}
                  onClick={() => onSelectStep(d.id)}
                  className="group flex flex-col items-center gap-1.5 cursor-pointer shrink-0"
                  style={{ width: deltaTrendItems.length <= 8 ? "64px" : "48px" }}
                >
                  {/* 柱顶:数值(增长)或剪刀(释放) */}
                  <div className="flex h-8 flex-col items-center justify-end gap-0.5">
                    {d.isNegative ? (
                      <>
                        <span className="text-amber-500" title="此步相对上一步释放了上下文(压缩/裁剪)">
                          <Scissors className="size-3" />
                        </span>
                        <span className="font-mono text-[10px] font-semibold text-amber-600 dark:text-amber-400">
                          −{d.magnitude}
                        </span>
                      </>
                    ) : (
                      <span className={cn(
                        "font-mono text-[10px] group-hover:text-foreground",
                        d.delta > 0 ? "font-semibold text-rose-500" : "text-muted-foreground"
                      )}>
                        {d.delta > 0 ? `+${d.delta}` : "±0"}
                      </span>
                    )}
                  </div>

                  {/* 相对基准线的柱体:增长向上(玫瑰),释放向下突破基准(琥珀) */}
                  <div className="relative flex w-full items-center justify-center" style={{ height: "72px" }}>
                    {/* 基准线 */}
                    <div className="absolute inset-x-0 top-1/2 h-px bg-border" />
                    {d.magnitude > 0 ? (
                      <div
                        style={{ height: `${heightPct}%` }}
                        className={cn(
                          "absolute inset-x-1 rounded-sm transition-all duration-200",
                          d.isNegative ? "bottom-1/2 bg-amber-400" : "top-1/2 bg-rose-400",
                          isSelected ? "ring-2 ring-primary" : "group-hover:opacity-80"
                        )}
                        title={d.isNegative ? `释放 ${d.magnitude} token` : `增长 +${d.magnitude} token`}
                      />
                    ) : null}
                  </div>

                  {/* 柱底标签 */}
                  <span className={cn(
                    "text-[10.5px] font-mono",
                    isSelected ? "font-bold text-primary" : "text-muted-foreground"
                  )}>
                    {d.label}
                  </span>
                </div>
              );
            })}
      </div>

      {/* Delta 模式图例说明 */}
      {trendMode === "delta" ? (
        <div className="flex flex-wrap items-center gap-x-4 text-[11px] text-muted-foreground">
          <span className="flex items-center gap-1">
            <span className="size-2.5 rounded-sm bg-rose-400" />
            <span>向上 = 相对上一步增长</span>
          </span>
          <span className="flex items-center gap-1">
            <span className="size-2.5 rounded-sm bg-amber-400" />
            <span>向下突破基准 = 压缩/裁剪释放(✂)</span>
          </span>
        </div>
      ) : null}

      {/* 单步白话速报卡片 (对标 DSH RequestDetail) */}
      {activeTrendStep ? (
        <div className="rounded-lg border bg-muted/20 p-2.5 flex flex-col gap-1.5 text-[12px]">
          <div className="flex items-center justify-between border-b border-border/60 pb-1 text-[11.5px]">
            <span className="font-semibold text-foreground flex items-center gap-1.5">
              <span>📌 单步速报 · {activeTrendStep.label}</span>
              <span className="rounded bg-muted px-1.5 py-0.2 font-mono text-[10.5px] text-muted-foreground">
                进 {activeTrendStep.tokens_in} · 出 {activeTrendStep.tokens_out} token
              </span>
            </span>
            <span className="text-muted-foreground text-[11px]">
              点击上方任意柱子可查看该步快照速报
            </span>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-2 pt-0.5 text-[11.5px]">
            {/* 1. 本轮提问 */}
            <div className="flex flex-col gap-0.5 bg-background/70 border rounded p-2">
              <span className="text-muted-foreground font-medium text-[11px] flex items-center gap-1">
                <User className="size-3 text-rose-500" />
                <span>【本轮】用户原始提问</span>
              </span>
              <p className="text-foreground leading-snug line-clamp-2">
                {activeTrendStep.question}
              </p>
            </div>

            {/* 2. 新增输入 */}
            <div className="flex flex-col gap-0.5 bg-background/70 border rounded p-2">
              <span className="text-muted-foreground font-medium text-[11px] flex items-center gap-1">
                <ArrowDownLeft className="size-3 text-sky-500" />
                <span>【输入】进入模型的内容</span>
              </span>
              <p className="text-muted-foreground leading-snug line-clamp-2">
                {activeTrendStep.lastInput}
              </p>
            </div>

            {/* 3. 本步回复 */}
            <div className="flex flex-col gap-0.5 bg-background/70 border rounded p-2">
              <span className="text-muted-foreground font-medium text-[11px] flex items-center gap-1">
                <ArrowUpRight className="size-3 text-purple-500" />
                <span>【回复】模型生成行为</span>
              </span>
              <p className="text-foreground font-mono leading-snug line-clamp-2">
                {activeTrendStep.responseSummary}
              </p>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
