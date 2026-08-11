/**
 * 思考过程折叠块：<think>...</think> 内容默认收起，点击展开。
 * 流式输出期间未闭合的 think 块保持展开（展示思考过程进行中）。
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Brain, ChevronDown } from "lucide-react";
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

/** 判断文本是否包含思考块（供流式消息决定是否按折叠渲染） */
export function hasThinkBlock(text: string): boolean {
  return text.includes("<think>");
}

export function ThinkBlock({ content, open }: { content: string; open?: boolean }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(open ?? false);
  // 流式未闭合块默认展开
  const isExpanded = expanded;

  return (
    <div className="my-2 overflow-hidden rounded-lg border bg-muted/40">
      <button
        type="button"
        onClick={() => setExpanded(!isExpanded)}
        className="flex w-full items-center gap-1.5 px-2.5 py-1.5 text-left text-xs text-muted-foreground transition-colors hover:bg-accent/50 hover:text-foreground"
      >
        <Brain size={13} className="shrink-0" />
        <span className="font-medium">{t("chat.think.process")}</span>
        {!isExpanded && content.length > 0 && (
          <span className="ml-1 truncate text-[10px] opacity-60">{content.slice(0, 40)}…</span>
        )}
        <ChevronDown
          size={13}
          className={cn("ml-auto shrink-0 transition-transform", isExpanded && "rotate-180")}
        />
      </button>
      {isExpanded && (
        <div className="max-h-64 overflow-y-auto whitespace-pre-wrap border-t px-3 py-2 text-xs leading-relaxed text-muted-foreground">
          {content}
        </div>
      )}
    </div>
  );
}
