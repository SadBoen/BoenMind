/**
 * 思考/工具执行过程折叠块：<think> 内容 + 工具调用绿条归入同一"过程"区，
 * 默认收起；正式答复（纯文本）在块下方，不显示绿条（对齐 pi-web 语义：
 * 过程可视化属于思考/执行阶段，用户要求"打开思考过程才能看到绿条"）。
 * 流式输出期间未闭合的 think 块保持展开（展示思考过程进行中）。
 */
import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Brain, ChevronDown, Wrench } from "lucide-react";
import { cn } from "@/lib/utils";

export interface ThinkSegment {
  type: "text" | "think";
  content: string;
  /** 仅流式未闭合的 think 块为 true（展开显示进行中的思考） */
  open?: boolean;
}

/**
 * 将文本解析为 文本/思考 分段。
 * 支持多个 think 块；流式时最后一个未闭合的 <think> 之后的内容视为思考内容。
 */
export function parseThinkBlocks(text: string): ThinkSegment[] {
  const segments: ThinkSegment[] = [];
  const re = /<think>([\s\S]*?)<\/think>/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    if (m.index > last) {
      segments.push({ type: "text", content: text.slice(last, m.index) });
    }
    segments.push({ type: "think", content: m[1] });
    last = m.index + m[0].length;
  }
  // 未闭合的 think（流式进行中）
  const openIdx = text.indexOf("<think>", last);
  if (openIdx >= 0) {
    if (openIdx > last) {
      segments.push({ type: "text", content: text.slice(last, openIdx) });
    }
    segments.push({ type: "think", content: text.slice(openIdx + 7), open: true });
  } else if (last < text.length) {
    segments.push({ type: "text", content: text.slice(last) });
  }
  return segments;
}

/** 折叠块内容区（思考段 + 工具块） */
export function ProcessBlock({
  thinks,
  toolCount,
  children,
  defaultOpen,
}: {
  /** 思考段落（可多个，按顺序展示） */
  thinks?: { content: string }[];
  /** 工具调用次数（标题后缀展示） */
  toolCount?: number;
  /** 工具调用块列表（展开区底部） */
  children?: ReactNode;
  /** 流式进行中默认展开 */
  defaultOpen?: boolean;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(defaultOpen ?? false);
  // 流式执行中 defaultOpen 由 false 变 true（工具开始执行）时自动展开；
  // 结束后不强制收起（用户已展开的保持展开）
  useEffect(() => {
    if (defaultOpen) setExpanded(true);
  }, [defaultOpen]);
  const hasTools = toolCount != null && toolCount > 0;
  const isExpanded = expanded;

  // 标题：有思考用"思考过程"，无思考纯工具用"工具执行"
  const title = thinks && thinks.length > 0 ? t("chat.think.process") : t("chat.think.toolsOnly");
  const preview = thinks?.find((s) => s.content.trim())?.content ?? "";
  const suffix = hasTools ? t("chat.think.withTools", { count: toolCount }) : "";

  return (
    <div className="my-2 overflow-hidden rounded-lg border bg-muted/40">
      <button
        type="button"
        onClick={() => setExpanded(!isExpanded)}
        className="flex w-full items-center gap-1.5 px-2.5 py-1.5 text-left text-xs text-muted-foreground transition-colors hover:bg-accent/50 hover:text-foreground"
      >
        {thinks && thinks.length > 0 ? (
          <Brain size={13} className="shrink-0" />
        ) : (
          <Wrench size={13} className="shrink-0" />
        )}
        <span className="font-medium">
          {title}
          {suffix && <span className="ml-1 opacity-70">{suffix}</span>}
        </span>
        {!isExpanded && preview.trim() && (
          <span className="ml-1 truncate text-[10px] opacity-60">{preview.slice(0, 40)}…</span>
        )}
        <ChevronDown
          size={13}
          className={cn("ml-auto shrink-0 transition-transform", isExpanded && "rotate-180")}
        />
      </button>
      {isExpanded && (
        <div className="border-t px-3 py-2">
          {thinks?.map((s, i) =>
            s.content.trim() ? (
              <div
                key={i}
                className="max-h-64 overflow-y-auto whitespace-pre-wrap text-xs leading-relaxed text-muted-foreground"
              >
                {s.content}
              </div>
            ) : null,
          )}
          {hasTools && (
            <div className={cn("flex flex-col gap-1.5", thinks?.some((s) => s.content.trim()) && "mt-2 border-t pt-2")}>
              {children}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
