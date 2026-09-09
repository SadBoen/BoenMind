import { useState } from "react";
import { Brain, ChevronDown, ChevronRight } from "lucide-react";

interface ThinkingBlockProps {
  text: string;
  isStreaming?: boolean;
  elapsedSeconds?: number;
}

/**
 * 思考块(2026-09-10 对齐 ZCode 样式,废除卡片/紫色/遮罩限高):
 * 1. 流式中 = 灰字"正在思考 · <思考尾部幽灵预览>",单行硬切无省略号,行尾右箭头
 * 2. 完成后 = "思考 · 持续了几秒"(有实测耗时报秒),默认折叠,点击展开
 * 3. 展开态 = 灰字全文挂 2px 左细线下,无高度限制、无渐变遮罩
 * 蓝本 token 见 .zcode/zcode-chat-replica.html(离线复刻留档)
 */
export function ThinkingBlock({ text, isStreaming = false, elapsedSeconds }: ThinkingBlockProps) {
  const [open, setOpen] = useState(false);

  const label = isStreaming
    ? "正在思考"
    : elapsedSeconds && elapsedSeconds > 0
      ? `思考 · 持续了 ${elapsedSeconds.toFixed(1)} 秒`
      : "思考 · 持续了几秒";
  // 幽灵预览取思考尾部约 160 字(按码点切,防劈开代理对),与 ZCode 流式观感一致
  const ghost = isStreaming ? Array.from(text).slice(-160).join("") : "";

  return (
    <div>
      <div
        className="zc-t clickable"
        data-slot="thinking-head"
        onClick={() => setOpen(!open)}
      >
        <Brain />
        <span className="shrink-0">{label}</span>
        {isStreaming ? (
          <>
            <span className="zc-ghost">
              <span className="sep">·</span>
              {ghost}
            </span>
            <ChevronRight className="zc-chev-r" />
          </>
        ) : open ? (
          <ChevronDown className="zc-chev open" />
        ) : (
          <ChevronRight className="zc-chev" />
        )}
      </div>
      {open && !isStreaming ? <div className="zc-sub zc-think">{text}</div> : null}
    </div>
  );
}
