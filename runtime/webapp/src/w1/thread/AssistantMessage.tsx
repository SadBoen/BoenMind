// 助手消息视图(#22 拆分:自 thread.tsx 机械移入):
// 解析后的思考链/工具树/终端块/正文渲染 + 重新生成(分支)/复制。
import { MessagePrimitive, useAuiState } from "@assistant-ui/react";
import { Bot, RotateCcw, Copy, Check } from "lucide-react";
import { useState } from "react";
import { useBoenmindApprovals } from "../runtime";
import { MarkdownRenderer } from "../MarkdownRenderer";
import { parseAssistantContent } from "../parser";
import { ThinkingBlock } from "../components/ThinkingBlock";
import { ToolTreeGroup } from "../components/ToolTreeGroup";
import { TerminalBlock } from "../components/TerminalBlock";

export function AssistantMessage() {
  const isRunning = useAuiState((s) => s.thread.isRunning);
  const messageIndex = useAuiState((s) => s.message.index);
  const { regenerateMessage } = useBoenmindApprovals();
  const [copied, setCopied] = useState(false);

  return (
    <MessagePrimitive.Root className="msg assistant">
      <div className="msg-header">
        <Bot size={13} className="text-accent" />
        <span>BoenMind Agent</span>
        {isRunning ? <span className="badge-tag">生成中…</span> : null}
      </div>
      <div className="content">
        <MessagePrimitive.Parts>
          {({ part }) => {
            if (part.type !== "text" || !part.text) return null;
            const blocks = parseAssistantContent(part.text, isRunning);
            return (
              // P1-29(2026-09-07 架构评审):key 用索引而非文本长度——
              // 长度作 key 会在流式期间每个 delta 都重挂载整棵子树,
              // ThinkingBlock 展开态/滚动位置被反复清零
              <div className="group/content flex flex-col gap-1.5" key="content">
                {blocks.map((b, idx) => {
                  if (b.type === "thinking") {
                    return (
                      <ThinkingBlock
                        key={idx}
                        text={b.text}
                        isStreaming={b.isStreaming}
                        elapsedSeconds={b.elapsedSeconds}
                      />
                    );
                  }
                  if (b.type === "explore_group" || b.type === "changes_group" || b.type === "generic_tool_group") {
                    return <ToolTreeGroup key={idx} block={b} />;
                  }
                  if (b.type === "terminal_block") {
                    return <TerminalBlock key={idx} item={b.item} />;
                  }
                  return (
                    <MarkdownRenderer key={idx} content={b.text} />
                  );
                })}
                <div className="msg-action-bar justify-start pt-1">
                  <button
                    className="msg-action-btn"
                    title="重新生成本条回复（分支）"
                    onClick={() => {
                      if (messageIndex !== undefined) {
                        void regenerateMessage(messageIndex);
                      }
                    }}
                  >
                    <RotateCcw size={12} />
                  </button>
                  <button
                    className="msg-action-btn"
                    title="复制回复"
                    onClick={async () => {
                      await navigator.clipboard.writeText(part.text);
                      setCopied(true);
                      setTimeout(() => setCopied(false), 1500);
                    }}
                  >
                    {copied ? <Check size={12} className="text-emerald-500" /> : <Copy size={12} />}
                  </button>
                </div>
              </div>
            );
          }}
        </MessagePrimitive.Parts>
      </div>
    </MessagePrimitive.Root>
  );
}
