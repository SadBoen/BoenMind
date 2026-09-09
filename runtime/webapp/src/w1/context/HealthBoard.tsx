//! 【第一层:健康度看板】(#22 拆分:自 context.tsx 视图块机械移入)
//! 模型窗口真实水位、输入/缓存/输出/速率、配方构成水杯与图例、
//! DSH 时间旅行横幅与遗忘轮数提醒。纯展示:数据经 props 注入。

import {
  Sparkles,
  CheckCircle2,
  Clock,
  AlertTriangle,
  ArrowDownLeft,
  ArrowUpRight,
  Zap,
  Gauge,
  TrendingUp,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { CtxStep } from "@/w2/api";
import { fmtDur } from "./utils";
import type { ParsedPromptRecipe } from "./utils";
import type { ContextStats } from "./types";

export function HealthBoard({
  stats,
  recipe,
  latestSnapshot,
  isTimeTraveling,
  timeTravelSnapshot,
  onExitTimeTravel,
  hoveredCategory,
  onHoverCategory,
}: {
  stats: ContextStats;
  recipe: ParsedPromptRecipe | null;
  latestSnapshot: CtxStep;
  isTimeTraveling: boolean;
  timeTravelSnapshot: CtxStep | null;
  onExitTimeTravel: () => void;
  hoveredCategory: string | null;
  onHoverCategory: (c: string | null) => void;
}) {
  return (
    <div className="bg-card rounded-xl border p-3.5 shadow-2xs flex flex-col gap-3">
      {/* DSH 时间旅行横幅:正浏览历史快照时明确提示,一键回到最新现场 */}
      {isTimeTraveling && timeTravelSnapshot ? (
        <div className="flex items-center justify-between rounded-lg border border-sky-500/40 bg-sky-500/10 px-3 py-1.5 text-[12px]">
          <span className="flex items-center gap-1.5 font-medium text-sky-700 dark:text-sky-300">
            <Clock className="size-3.5" />
            <span>
              🕰️ 时间旅行中 · 正在回看第 {timeTravelSnapshot.turn_index} 轮 · 第 {timeTravelSnapshot.step} 步
              (seq {timeTravelSnapshot.seq}) 的历史快照,以下全部卡片均为当时的真实装配
            </span>
          </span>
          <Button
            size="sm"
            variant="outline"
            className="h-6 gap-1 px-2 text-[11px]"
            onClick={onExitTimeTravel}
          >
            <Sparkles className="size-3" />
            <span>回到最新现场 (Live)</span>
          </Button>
        </div>
      ) : null}
      <div className="flex flex-wrap items-center justify-between gap-2 border-b pb-2">
        {/* 窗口水位:仅在用户登记过窗口容量时出真实进度;否则如实「未知」 */}
        <div className="flex items-center gap-2">
          <Gauge className="size-4 text-primary" />
          <span className="text-[13px] font-semibold text-foreground">
            模型窗口水位
          </span>
          {stats.maxWindow != null ? (
            <>
              <span className="rounded-md bg-muted px-2 py-0.5 font-mono text-[11px] font-medium text-foreground">
                {stats.realTokensIn + stats.realTokensOut} / {stats.maxWindow.toLocaleString()} token ({stats.headroomPct}%)
              </span>
              <span className={cn(
                "flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium",
                (stats.headroomPct ?? 0) >= 80 ? "bg-[var(--state-error-bg)] text-[var(--state-error-fg)]" : (stats.headroomPct ?? 0) >= 50 ? "bg-[var(--state-warn-bg)] text-[var(--state-warn-fg)]" : "bg-[var(--state-success-bg)] text-[var(--state-success-fg)]"
              )}>
                <CheckCircle2 className="size-3" />
                <span>剩余安全余量: {stats.remainingHeadroom?.toLocaleString()} token</span>
              </span>
            </>
          ) : (
            <>
              <span className="rounded-md bg-muted px-2 py-0.5 font-mono text-[11px] font-medium text-foreground">
                本轮进出共 {stats.realTokensIn + stats.realTokensOut} token
              </span>
              <span className="flex items-center gap-1 rounded-full bg-[var(--state-warn-bg)] px-2 py-0.5 text-[11px] font-medium text-[var(--state-warn-fg)]" title="未登记该模型的上下文窗口容量,无法计算水位占比;在「设置 → 模型提供商」模型清单里登记窗口 token 数后即可见血条">
                <AlertTriangle className="size-3" />
                <span>窗口容量未登记,无法计算水位(可在设置里补登记)</span>
              </span>
            </>
          )}
        </div>

        {/* 输入、缓存、输出、生成速率与耗时(未上报如实标注) */}
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[12px] text-muted-foreground">
          <span className="flex items-center gap-1">
            <ArrowDownLeft className="size-3.5 text-sky-500" />
            输入: <strong className="text-foreground">{stats.realTokensIn} token</strong>
          </span>
          <span>·</span>
          <span className="flex items-center gap-1" title={stats.cachedTokens == null ? "提供商未上报提示词缓存命中明细" : "Provider 服务端提示词缓存命中"}>
            <Zap className="size-3.5 text-amber-500" />
            缓存: <strong className={stats.cachedTokens != null && stats.cachedTokens > 0 ? "text-emerald-600 dark:text-emerald-400" : "text-foreground"}>
              {stats.cachedTokens == null ? "未上报" : `${stats.cachedTokens} token`}
            </strong>
          </span>
          <span>·</span>
          <span className="flex items-center gap-1">
            <ArrowUpRight className="size-3.5 text-purple-500" />
            输出: <strong className="text-foreground">{stats.realTokensOut} token</strong>
          </span>
          <span>·</span>
          <span className="flex items-center gap-1" title="输出 token ÷ 全程耗时(含首字排队等待)">
            <TrendingUp className="size-3.5 text-emerald-500" />
            速率: <strong className="text-foreground">{stats.speed} token/s</strong>
          </span>
          {stats.ttftMs != null ? (
            <>
              <span>·</span>
              <span className="flex items-center gap-1" title="请求发出到第一个字回来的时间(仅流式可测)">
                <Zap className="size-3.5 text-sky-500" />
                首字: <strong className="text-foreground">{fmtDur(stats.ttftMs)}</strong>
              </span>
            </>
          ) : null}
          <span>·</span>
          <span className="flex items-center gap-1">
            <Clock className="size-3.5" />
            耗时: <strong className="text-foreground">{fmtDur(latestSnapshot.latency_ms)}</strong>
          </span>
        </div>
      </div>

      {/* 进度条水杯 */}
      <div>
        <div className="mb-1.5 flex items-center justify-between text-[11.5px] text-muted-foreground">
          <span>输入内容配方构成：</span>
          <span>
            当前会话保留 {recipe?.historyTurns.length ?? 0}/20 轮记忆
            {(stats?.evictedTurns ?? 0) > 0 ? (
              <span className="text-[var(--state-warn-fg)]">
                （最早 {stats?.evictedTurns} 轮已被自动遗忘）
              </span>
            ) : null}
          </span>
        </div>
        <div className="flex h-3 w-full overflow-hidden rounded-full bg-muted/80">
          {stats.pct.persona > 0 ? (
            <div
              style={{ width: `${stats.pct.persona}%` }}
              className="bg-indigo-500 transition-all hover:opacity-80"
              title={`AI人设与规矩: 约 ${stats.personaTokens} token (${stats.pct.persona}%)`}
            />
          ) : null}
          {stats.pct.skills > 0 ? (
            <div
              style={{ width: `${stats.pct.skills}%` }}
              className="bg-purple-500 transition-all hover:opacity-80"
              title={`携带特长技能: 约 ${stats.skillsTokens} token (${stats.pct.skills}%)`}
            />
          ) : null}
          {stats.pct.tools > 0 ? (
            <div
              style={{ width: `${stats.pct.tools}%` }}
              className="bg-amber-500 transition-all hover:opacity-80"
              title={`装备工具箱: 约 ${stats.toolsTokens} token (${stats.pct.tools}%)`}
            />
          ) : null}
          {stats.pct.history > 0 ? (
            <div
              style={{ width: `${stats.pct.history}%` }}
              className="bg-sky-500 transition-all hover:opacity-80"
              title={`之前聊天记忆: 约 ${stats.historyTokens} token (${stats.pct.history}%)`}
            />
          ) : null}
          {stats.pct.ws > 0 ? (
            <div
              style={{ width: `${stats.pct.ws}%` }}
              className="bg-emerald-500 transition-all hover:opacity-80"
              title={`工作区环境: 约 ${stats.wsTokens} token (${stats.pct.ws}%)`}
            />
          ) : null}
          {stats.pct.input > 0 ? (
            <div
              style={{ width: `${stats.pct.input}%` }}
              className="bg-rose-500 transition-all hover:opacity-80"
              title={`本次提问: 约 ${stats.inputTokens} token (${stats.pct.input}%)`}
            />
          ) : null}
        </div>

        {/* 图例对照表 (支持 Hover 联动高亮趋势图中的对应切片) */}
        <div className="mt-2.5 flex flex-wrap items-center gap-x-4 gap-y-1 text-[11.5px]">
          <span
            onMouseEnter={() => onHoverCategory("persona")}
            onMouseLeave={() => onHoverCategory(null)}
            className={cn("flex items-center gap-1.5 cursor-pointer rounded px-1.5 py-0.5 transition-colors", hoveredCategory === "persona" ? "bg-indigo-500/15 ring-1 ring-indigo-500" : "hover:bg-muted")}
          >
            <span className="size-2.5 rounded-full bg-indigo-500" />
            <span className="text-foreground">🎭 人设规矩:</span>
            <span className="text-muted-foreground">{stats.pct.persona}% ({stats.personaTokens} token)</span>
          </span>
          {stats.skillsTokens > 0 ? (
            <span
              onMouseEnter={() => onHoverCategory("skills")}
              onMouseLeave={() => onHoverCategory(null)}
              className={cn("flex items-center gap-1.5 cursor-pointer rounded px-1.5 py-0.5 transition-colors", hoveredCategory === "skills" ? "bg-purple-500/15 ring-1 ring-purple-500" : "hover:bg-muted")}
            >
              <span className="size-2.5 rounded-full bg-purple-500" />
              <span className="text-foreground">⚡ 特长技能:</span>
              <span className="text-muted-foreground">{stats.pct.skills}% ({stats.skillsTokens} token)</span>
            </span>
          ) : null}
          <span
            onMouseEnter={() => onHoverCategory("tools")}
            onMouseLeave={() => onHoverCategory(null)}
            className={cn("flex items-center gap-1.5 cursor-pointer rounded px-1.5 py-0.5 transition-colors", hoveredCategory === "tools" ? "bg-amber-500/15 ring-1 ring-amber-500" : "hover:bg-muted")}
          >
            <span className="size-2.5 rounded-full bg-amber-500" />
            <span className="text-foreground">🛠️ 工具背包:</span>
            <span className="text-muted-foreground">{stats.pct.tools}% ({stats.toolsTokens} token)</span>
          </span>
          <span
            onMouseEnter={() => onHoverCategory("history")}
            onMouseLeave={() => onHoverCategory(null)}
            className={cn("flex items-center gap-1.5 cursor-pointer rounded px-1.5 py-0.5 transition-colors", hoveredCategory === "history" ? "bg-sky-500/15 ring-1 ring-sky-500" : "hover:bg-muted")}
          >
            <span className="size-2.5 rounded-full bg-sky-500" />
            <span className="text-foreground">💬 聊天记忆:</span>
            <span className="text-muted-foreground">{stats.pct.history}% ({stats.historyTokens} token)</span>
          </span>
          {stats.wsTokens > 0 ? (
            <span
              onMouseEnter={() => onHoverCategory("ws")}
              onMouseLeave={() => onHoverCategory(null)}
              className={cn("flex items-center gap-1.5 cursor-pointer rounded px-1.5 py-0.5 transition-colors", hoveredCategory === "ws" ? "bg-emerald-500/15 ring-1 ring-emerald-500" : "hover:bg-muted")}
            >
              <span className="size-2.5 rounded-full bg-emerald-500" />
              <span className="text-foreground">📁 电脑目录:</span>
              <span className="text-muted-foreground">{stats.pct.ws}% ({stats.wsTokens} token)</span>
            </span>
          ) : null}
          <span
            onMouseEnter={() => onHoverCategory("input")}
            onMouseLeave={() => onHoverCategory(null)}
            className={cn("flex items-center gap-1.5 cursor-pointer rounded px-1.5 py-0.5 transition-colors", hoveredCategory === "input" ? "bg-rose-500/15 ring-1 ring-rose-500" : "hover:bg-muted")}
          >
            <span className="size-2.5 rounded-full bg-rose-500" />
            <span className="text-foreground">❓ 您的问题:</span>
            <span className="text-muted-foreground">{stats.pct.input}% ({stats.inputTokens} token)</span>
          </span>
        </div>
      </div>
    </div>
  );
}
