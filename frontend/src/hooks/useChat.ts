import { useCallback, useEffect, useRef, useState } from "react";
import { rpc } from "../client";

export interface ToolBlock {
  type: "tool-call" | "tool-result";
  name?: string;
  callId?: string;
  arguments?: string;
  output?: string;
  isError?: boolean;
}

export interface Message {
  id: string;
  role: "user" | "assistant";
  text: string;
  pending?: boolean;
  blocks?: ToolBlock[];
}

// dsh wire 事件（web-server translate 后，history 与 WS 同形）。
// history: { events: [{ event: { type, seq, data } }] }
// WS mux:  { type:"server-request", method:"session/event", payload:{ sessionId, event } }
interface WireEvent {
  type: string;
  seq: number;
  data?: {
    content?: { type?: string; text?: string }[];
    message?: { content?: { type?: string; text?: string }[]; role?: string };
    chunk?: { text?: string };
    [k: string]: unknown;
  };
}
interface WireRecord {
  type: string;
  method?: string;
  payload?: { sessionId?: string; event?: WireEvent };
}

export function useChat(sessionId: string | null) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const wsRef = useRef<WebSocket | null>(null);

  // 切换会话：清空并加载历史（session.history → wire events 数组）。
  useEffect(() => {
    setMessages([]);
    setError(null);
    if (!sessionId) return;
    rpc<{ events: { event: WireEvent }[] }>("session.history", { sessionId })
      .then((h) => {
        const msgs: Message[] = [];
        for (const { event } of h.events) {
          if (event.type === "user/message") {
            msgs.push({ id: crypto.randomUUID(), role: "user", text: eventText(event) });
          } else if (event.type === "assistant/message") {
            msgs.push({ id: crypto.randomUUID(), role: "assistant", text: eventText(event) });
          } else if (event.type === "tool/call") {
            const d = event.data ?? {};
            msgs.push({
              id: crypto.randomUUID(),
              role: "assistant",
              text: "",
              blocks: [
                {
                  type: "tool-call",
                  name: (d.name as string) ?? "",
                  callId: (d.callId as string) ?? "",
                  arguments: (d.arguments as string) ?? "",
                },
              ],
            });
          } else if (event.type === "tool/result") {
            const d = event.data ?? {};
            const callId = (d.callId as string) ?? "";
            const msg = d.message as { content?: any[] } | undefined;
            const block = msg?.content?.find((c) => c.type === "tool-result");
            const textBlocks = Array.isArray(block?.content)
              ? block.content.map((c: any) => c.text ?? "").join("")
              : "";
            attachToolResult(msgs, callId, textBlocks, block?.isError === true);
          }
        }
        setMessages(msgs);
      })
      .catch((e) => setError(e.message));
  }, [sessionId]);

  // WS 事件流：增量渲染（user/message + assistant/chunk + assistant/message）。
  useEffect(() => {
    if (!sessionId) return;
    const ws = new WebSocket(`ws://${location.host}/api/events.mux`);
    wsRef.current = ws;
    ws.onmessage = (e) => {
      let rec: WireRecord;
      try {
        rec = JSON.parse(e.data as string);
      } catch {
        return;
      }
      if (rec.method !== "session/event") return;
      const { sessionId: sid, event: ev } = rec.payload ?? {};
      if (!sid || !ev || sid !== sessionId) return;
      if (ev.type === "user/message") {
        setMessages((m) => [...m, { id: crypto.randomUUID(), role: "user", text: eventText(ev) }]);
      } else if (ev.type === "assistant/chunk") {
        setStreaming(true);
        const delta = ev.data?.chunk?.text ?? "";
        setMessages((m) => {
          const last = m[m.length - 1];
          if (last && last.role === "assistant") {
            const next = [...m];
            next[next.length - 1] = { ...last, text: last.text + delta, pending: true };
            return next;
          }
          return [...m, { id: crypto.randomUUID(), role: "assistant", text: delta, pending: true }];
        });
      } else if (ev.type === "tool/call") {
        const d = ev.data ?? {};
        setMessages((m) => [
          ...m,
          {
            id: crypto.randomUUID(),
            role: "assistant",
            text: "",
            blocks: [
              {
                type: "tool-call",
                name: (d.name as string) ?? "",
                callId: (d.callId as string) ?? "",
                arguments: (d.arguments as string) ?? "",
              },
            ],
          },
        ]);
      } else if (ev.type === "tool/result") {
        const d = ev.data ?? {};
        const callId = (d.callId as string) ?? "";
        const msg = d.message as { content?: any[] } | undefined;
        const block = msg?.content?.find((c) => c.type === "tool-result");
        const textBlocks = Array.isArray(block?.content)
          ? block.content.map((c: any) => c.text ?? "").join("")
          : "";
        setMessages((m) => {
          const next = [...m];
          attachToolResult(next, callId, textBlocks, block?.isError === true);
          return next;
        });
      } else if (ev.type === "assistant/message") {
        const text = eventText(ev);
        setMessages((m) => {
          const last = m[m.length - 1];
          if (last && last.role === "assistant" && last.pending) {
            const next = [...m];
            next[next.length - 1] = { ...last, text, pending: false };
            return next;
          }
          return [...m, { id: crypto.randomUUID(), role: "assistant", text }];
        });
        setStreaming(false);
      }
    };
    ws.onerror = () => setError("WS 连接失败");
    return () => ws.close();
  }, [sessionId]);

  const send = useCallback(
    async (text: string) => {
      if (!sessionId || !text.trim() || streaming) return;
      setStreaming(true);
      setError(null);
      try {
        await rpc("session.prompt", {
          sessionId,
          content: [{ type: "text", text }],
        });
      } catch (e) {
        setError((e as Error).message);
        setStreaming(false);
      }
    },
    [sessionId, streaming]
  );

  return { messages, streaming, error, send };
}

// 从 wire 事件取文本：user/message → data.content；assistant/message → data.message.content。
function eventText(ev: WireEvent): string {
  const blocks = ev.data?.message?.content ?? ev.data?.content ?? [];
  return blocks
    .filter((b) => b.text !== undefined)
    .map((b) => b.text as string)
    .join("");
}

// 把 tool 结果挂到同 callId 的 tool-call 卡片上（跨消息：从后往前找最后一条
// 未填充结果的 tool-call 块）。找不到则忽略（历史已重建/异常序）。
function attachToolResult(msgs: Message[], callId: string, output: string, isError: boolean) {
  for (let i = msgs.length - 1; i >= 0; i--) {
    const blocks = msgs[i].blocks;
    if (!blocks) continue;
    for (let j = blocks.length - 1; j >= 0; j--) {
      const b = blocks[j];
      if (b.type === "tool-call" && (!b.callId || b.callId === callId) && b.output === undefined) {
        blocks[j] = { ...b, output, isError };
        return;
      }
    }
  }
}
