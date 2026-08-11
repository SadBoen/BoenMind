/**
 * 工具调用块：仿 pi-web —— 折叠展示工具名 + 参数预览。
 * 颜色语义：成功绿色 / 报错红色 / 执行中中性灰。点击展开看完整参数 JSON。
 * 注：按产品约定不展示工具执行结果内容，仅以颜色区分成败。
 */
import { memo, useState } from "react";
import { ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";

export interface ToolCallView {
  tool_name: string;
  args: unknown;
  is_error?: boolean;
  /** 流式执行中（颜色取中性灰，结束后按 is_error 定色） */
  running?: boolean;
}

/** 参数单行预览：紧凑 JSON，超长省略 */
function argsPreview(args: unknown): string {
  const text = JSON.stringify(args ?? {});
  return text.length > 120 ? `${text.slice(0, 120)}…` : text;
}

export const ToolCallBlock = memo(function ToolCallBlock({ call }: { call: ToolCallView }) {
  const [expanded, setExpanded] = useState(false);
  const argsText = JSON.stringify(call.args ?? {}, null, 2);
  const running = call.running ?? false;

  // 成功绿 / 报错红 / 执行中中性 —— 与 pi-web 一致（emerald-600 / red-500）
  const tone = running
    ? { text: "text-muted-foreground", border: "border-border", bg: "bg-muted/40" }
    : call.is_error
      ? { text: "text-red-500", border: "border-red-500/45", bg: "bg-red-500/5" }
      : { text: "text-emerald-600 dark:text-emerald-500", border: "border-emerald-600/25", bg: "bg-emerald-600/5" };

  return (
    <div className={cn("overflow-hidden rounded-lg border text-xs", tone.border, tone.bg)}>
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left"
      >
        <span className={cn("shrink-0 font-mono text-[11px] font-semibold", tone.text)}>
          {call.tool_name}
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground/80">
          {argsPreview(call.args)}
        </span>
        {running && (
          <span className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-muted-foreground/60" />
        )}
        <ChevronDown
          size={12}
          className={cn("shrink-0 text-muted-foreground/60 transition-transform", expanded && "rotate-180")}
        />
      </button>
      {expanded && (
        <pre
          className={cn(
            "m-0 whitespace-pre-wrap break-all border-t px-2.5 py-2 font-mono text-[11px] leading-relaxed",
            tone.border,
            "bg-background/40 text-muted-foreground",
          )}
        >
          {argsText}
        </pre>
      )}
    </div>
  );
});
