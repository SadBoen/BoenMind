// W1(ADR-0014):ExternalStore 运行时接线——壳子状态 ↔ /v1/chat/completions
// 合同:见 milestones/W1-implementation-spec.md §4/§5
import {
  AssistantRuntimeProvider,
  useExternalStoreRuntime,
} from "@assistant-ui/react";
import type { AppendMessage, ThreadMessageLike } from "@assistant-ui/react";
import { useEffect, useState } from "react";

type TextPart = { type: "text"; text: string };

export function BoenmindRuntimeProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const [messages, setMessages] = useState<ThreadMessageLike[]>([]);
  const [isRunning, setIsRunning] = useState(false);

  const sendUserText = async (text: string) => {
    setIsRunning(true);
    setMessages((cur) => [
      ...cur,
      { role: "user", content: [{ type: "text", text }] as TextPart[] },
    ]);
    // 流式助手消息:占位空文本,随 SSE delta 增量拼接(不变式:每次整体替换
    // 最后一条消息,保证 external-store 转换器看到新对象标识而重渲染)
    const assistant: ThreadMessageLike = {
      role: "assistant",
      content: [{ type: "text", text: "" }] as TextPart[],
    };
    setMessages((cur) => [...cur, assistant]);
    const appendDelta = (delta: string) => {
      setMessages((cur) => {
        if (cur.length === 0) return cur;
        const copy = [...cur];
        const last = copy[copy.length - 1];
        const parts = (last.content as TextPart[]).map((p) => ({ ...p }));
        parts[0] = { ...parts[0], text: parts[0].text + delta };
        copy[copy.length - 1] = { ...last, content: parts };
        return copy;
      });
    };

    try {
      const doFetch = (withSession: boolean) => {
        const headers: Record<string, string> = {
          "Content-Type": "application/json",
        };
        const sid = withSession ? localStorage.getItem("bm_session") : null;
        if (sid) headers["X-Bm-Session"] = sid;
        return fetch("/v1/chat/completions", {
          method: "POST",
          headers,
          body: JSON.stringify({
            model: "auto",
            stream: true,
            messages: [{ role: "user", content: text }],
          }),
        });
      };
      let res = await doFetch(true);
      // 服务器重启会清空内存会话表:400「未知会话」→ 清记忆重开新会话重试一次
      if (res.status === 400) {
        localStorage.removeItem("bm_session");
        res = await doFetch(false);
      }
      if (!res.ok || !res.body) {
        const detail = await res.text().catch(() => "");
        throw new Error(`HTTP ${res.status} ${detail.slice(0, 160)}`);
      }
      const newSid = res.headers.get("x-bm-session");
      if (newSid) localStorage.setItem("bm_session", newSid);

      const reader = res.body.getReader();
      const dec = new TextDecoder();
      let buf = "";
      stream: while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += dec.decode(value, { stream: true });
        const blocks = buf.split("\n\n");
        buf = blocks.pop() ?? "";
        for (const block of blocks) {
          const line = block.trim();
          if (!line.startsWith("data: ")) continue;
          const data = line.slice(6);
          if (data === "[DONE]") break stream;
          const v = JSON.parse(data) as {
            choices?: Array<{ delta?: { content?: string } }>;
          };
          const d = v.choices?.[0]?.delta?.content;
          if (typeof d === "string" && d) appendDelta(d);
        }
      }
    } catch (e) {
      appendDelta(`\n[连接失败: ${e instanceof Error ? e.message : String(e)}]`);
    } finally {
      setIsRunning(false);
    }
  };

  const onNew = async (message: AppendMessage) => {
    const first = message.content[0];
    if (first?.type !== "text") throw new Error("W1 仅支持文本输入");
    await sendUserText(first.text);
  };

  // E2E 钩子(W1 回归入口):?e2e=<文本> 装载后自动发送一次,供自动化
  // 浏览器测试(输入自动化不稳时的确定性通道);不影响手工使用
  useEffect(() => {
    const q = new URLSearchParams(window.location.search).get("e2e");
    if (!q) return;
    const t = setTimeout(() => sendUserText(q), 800);
    return () => clearTimeout(t);
  }, []);

  useEffect(() => {
    document.title = "BM n=" + messages.length + " run=" + isRunning;
  }, [messages, isRunning]);

  const runtime = useExternalStoreRuntime({
    messages,
    setMessages,
    onNew,
    isRunning,
    convertMessage: (m) => m,
  });

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      {children}
    </AssistantRuntimeProvider>
  );
}
