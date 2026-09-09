//! TAB 6: 步骤时序流与深度思考链(轨迹视图;#22 拆分:自 context.tsx 机械移入)
//! tool_call/tool_result/assistant_final/turn_end 事件树 + 思考链卡片。

import { Activity, Brain } from "lucide-react";
import { cn } from "@/lib/utils";
import type { CtxStep } from "@/w2/api";
import { fmtDur, type ParsedPromptRecipe } from "../utils";
import type { ContextStats } from "../types";

export function TrajectoryTab({
  visible,
  recipe,
  stats,
}: {
  visible: CtxStep[];
  recipe: ParsedPromptRecipe;
  stats: ContextStats | null;
}) {
  return (
    <div className="flex flex-col gap-3 rounded-xl border bg-card p-3.5 shadow-2xs">
      <div className="flex flex-wrap items-center justify-between border-b pb-2 text-[13px] font-semibold text-foreground">
        <span className="flex items-center gap-1.5">
          <Activity className="size-4 text-primary" />
          <span>交互执行细节与模型深度思考链 (Thinking Chain)</span>
        </span>
      </div>

      {/* 深度思考链卡片:正文片段如实展示;token 分账只标提供商实报,
          未上报则标口径(条目内文本粗估,不冒充真实数) */}
      {recipe.reasoningSnippet ? (
        <div className="rounded-lg border border-purple-500/40 bg-purple-500/10 p-3">
          <div className="mb-1 flex items-center justify-between text-[12px] font-semibold text-purple-600 dark:text-purple-400">
            <span className="flex items-center gap-1.5">
              <Brain className="size-4" />
              <span>
                🧠 模型深度思考链 (
                {stats?.reasoningTokens != null
                  ? `Reasoning Tokens ${stats.reasoningTokens} token·提供商实报`
                  : stats?.reasoningSnippetEstimated != null
                    ? `片段约 ${stats.reasoningSnippetEstimated} token·按文本粗估,提供商未上报分账`
                    : "提供商未上报分账"}
                )
              </span>
            </span>
            <span className="text-[11px] font-mono text-muted-foreground">thinking_content</span>
          </div>
          <pre className="max-h-48 overflow-auto rounded bg-background/80 p-2.5 font-mono text-[11px] leading-relaxed text-foreground whitespace-pre-wrap break-all">
            {recipe.reasoningSnippet}
          </pre>
        </div>
      ) : null}

      {visible.length === 0 ? (
        <div className="py-6 text-center text-[12.5px] text-muted-foreground">
          暂无步骤事件
        </div>
      ) : (
        visible.map((s) => {
          if (s.kind) {
            const d = (s.data ?? {}) as Record<string, unknown>;
            const t = (() => {
              const dd = new Date(s.ts);
              return isNaN(dd.getTime()) ? s.ts : dd.toLocaleTimeString();
            })();

            const evMap: Record<string, { label: string; color: string; desc: string }> = {
              tool_call: {
                label: "AI 决定使用工具",
                color: "text-amber-500 bg-amber-500/10 border-amber-500/20",
                desc: `调用了 ${String(d.tool ?? "")}，参数为 ${JSON.stringify(d.arguments ?? {})}`,
              },
              tool_result: {
                label: "工具完成并反馈",
                color: "text-sky-500 bg-sky-500/10 border-sky-500/20",
                desc: `耗时 ${fmtDur(d.elapsed_ms as number)}，返回结果已回喂给 AI`,
              },
              assistant_final: {
                label: "AI 组织最终答复",
                color: "text-purple-500 bg-purple-500/10 border-purple-500/20",
                desc: `生成了答复，输出消耗约 ${String(d.tokens_out ?? "—")} token`,
              },
              turn_end: {
                label: "交互完满结束",
                color: "text-emerald-500 bg-emerald-500/10 border-emerald-500/20",
                desc: `本次对话顺利完成，总耗时 ${fmtDur(d.latency_ms as number)}`,
              },
            };

            const meta = evMap[s.kind] ?? {
              label: s.kind,
              color: "text-muted-foreground bg-muted border-border",
              desc: "",
            };

            return (
              <div
                key={s.seq}
                className="flex items-start gap-3 rounded-lg border p-2.5 bg-muted/20 text-[12px]"
              >
                <span className={cn("rounded-md px-2 py-0.5 text-[11px] font-medium border shrink-0", meta.color)}>
                  {meta.label}
                </span>
                <div className="flex-1 min-w-0">
                  <div className="text-foreground font-medium">{meta.desc}</div>
                  {d.result || d.content ? (
                    <pre className="mt-1.5 max-h-32 overflow-auto rounded bg-background/80 p-2 font-mono text-[11px] text-muted-foreground whitespace-pre-wrap break-all">
                      {String(d.result ?? d.content ?? "")}
                    </pre>
                  ) : null}
                </div>
                <span className="text-[11px] text-muted-foreground font-mono shrink-0">
                  {t}
                </span>
              </div>
            );
          }
          return null;
        })
      )}
    </div>
  );
}
