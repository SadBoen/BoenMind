// 助手消息视图(#22 拆分:自 thread.tsx 机械移入):
// 解析后的思考链/工具树/终端块/正文渲染 + 重新生成(分支)/复制。
// ZCode 回合头口径——运行中灰色"工作中 X 分 X 秒",完成后加粗"已工作 …"
// (可折叠整回合),流式时内容尾部挂旋转 spinner。蓝本 token 见
// .zcode/zcode-chat-replica.html(离线复刻留档)。
import { MessagePrimitive, useAuiState } from "@assistant-ui/react";
import { RotateCcw, Copy, Check, ChevronDown, ChevronRight, Loader } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useBoenmindApprovals } from "../runtime";
import { MarkdownRenderer } from "../MarkdownRenderer";
import { parseAssistantContent, toolBlocksFromEvents } from "../parser";
import { ThinkingBlock } from "../components/ThinkingBlock";
import { ToolTreeGroup } from "../components/ToolTreeGroup";
import { TerminalBlock } from "../components/TerminalBlock";

function formatDuration(totalSeconds: number): string {
  const m = Math.floor(totalSeconds / 60);
  const s = totalSeconds % 60;
  return m > 0 ? `${m} 分 ${s} 秒` : `${s} 秒`;
}

export function AssistantMessage() {
  const isRunning = useAuiState((s) => s.thread.isRunning);
  const messageIndex = useAuiState((s) => s.message.index);
  // ADR-0055:工具事件属「当前回合」;只在最后一条助手消息上渲染(历史回放
  // 不含工具事件,与旧行为一致——工具组本就仅实时可见)。
  const isLast = useAuiState(
    (s) => s.message.index === s.thread.messages.length - 1,
  );
  const { regenerateMessage } = useBoenmindApprovals();
  const [copied, setCopied] = useState(false);
 // 回合计时:起始挂在本消息首次进入运行态的时刻,完成即冻结。
 // 历史回放的消息没有计时起点,不渲染回合头(无时间可报,不谎报)。
  const startRef = useRef<number | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [doneSeconds, setDoneSeconds] = useState<number | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  // ADR-0055:工具调用来自结构化事件(不再解析正文标记)
  const { toolEvents } = useBoenmindApprovals();

  useEffect(() => {
    if (!isRunning) {
      if (startRef.current !== null) {
        setDoneSeconds(
          Math.max(1, Math.round((Date.now() - startRef.current) / 1000)),
        );
        startRef.current = null;
      }
      return;
    }
    if (startRef.current === null) startRef.current = Date.now();
    const iv = window.setInterval(() => {
      setElapsed(
        Math.max(
          1,
          Math.round((Date.now() - (startRef.current ?? Date.now())) / 1000),
        ),
      );
    }, 1000);
    return () => window.clearInterval(iv);
  }, [isRunning]);

  return (
    <MessagePrimitive.Root className="msg assistant">
      {doneSeconds !== null ? (
        <div
          className="zc-run-head done"
          data-slot="run-head"
          onClick={() => setCollapsed((c) => !c)}
        >
          <span>已工作 {formatDuration(doneSeconds)}</span>
          {collapsed ? (
            <ChevronRight className="zc-chev" />
          ) : (
            <ChevronDown className="zc-chev open" />
          )}
        </div>
      ) : null}
      {isRunning && elapsed > 0 ? (
        <div className="zc-run-head running" data-slot="run-head-running">
          <span>工作中 {formatDuration(elapsed)}</span>
        </div>
      ) : null}
      {collapsed ? null : (
        <div className="content">
          <MessagePrimitive.Parts>
            {({ part }) => {
              if (part.type !== "text" || !part.text) {
                return isRunning ? <Loader className="zc-spin" /> : null;
              }
              const blocks = parseAssistantContent(part.text, isRunning);
              // ADR-0055:本回合工具调用(结构化事件)附于正文之后渲染。
              const toolBlocks = isLast ? toolBlocksFromEvents(toolEvents) : [];
              return (
 // P1-29():key 用索引而非文本长度——
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
                  {toolBlocks.map((b, idx) => {
                    if (b.type === "explore_group" || b.type === "changes_group" || b.type === "generic_tool_group") {
                      return <ToolTreeGroup key={`t${idx}`} block={b} />;
                    }
                    if (b.type === "terminal_block") {
                      return <TerminalBlock key={`t${idx}`} item={b.item} />;
                    }
                    return null;
                  })}
                  {isRunning ? <Loader className="zc-spin" /> : null}
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
      )}
    </MessagePrimitive.Root>
  );
}
