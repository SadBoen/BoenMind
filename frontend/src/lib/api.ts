// 后端 RPC 客户端（对齐 web-server 契约：/api/{endpoint} POST client-request 信封；
// 实时事件走 /api/events.mux WS 下行，见 useMuxStream）。
import type { ChatMessage, ToolCall } from "../types";
import { uid } from "./format";

export interface RpcOk<T> {
  ok: true;
  value: T;
}
export interface RpcErr {
  ok: false;
  error: { code: string; message: string; details: Record<string, unknown> };
}
export type RpcResult<T> = RpcOk<T> | RpcErr;

export interface WireEvent {
  type: string;
  seq: number;
  time: number;
  surfaceOp?: "append" | "replace";
  data: Record<string, unknown>;
}

export interface WireBlock {
  type: string;
  text?: string;
  [k: string]: unknown;
}

let rpcSeq = 0;

/** RPC 超时：后端挂起时 fetch 不再永久 pending（流式内容走 WS，不受此影响）。 */
const RPC_TIMEOUT_MS = 30000;

export async function rpc<T = unknown>(endpoint: string, payload: unknown): Promise<RpcResult<T>> {
  const rpcId = `webui-${Date.now().toString(36)}-${(++rpcSeq).toString(36)}`;
  const res = await fetch(`/api/${endpoint}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ type: "client-request", rpcId, method: endpoint, payload }),
    signal: AbortSignal.timeout(RPC_TIMEOUT_MS),
  });
  const text = await res.text();
  let data: { result?: RpcResult<T> } | null = null;
  try {
    data = JSON.parse(text);
  } catch {
    // 载体层错误响应（非 JSON）——转成统一错误面。
    return { ok: false, error: { code: "http-error", message: `${res.status} ${text || res.statusText}`, details: {} } };
  }
  if (!data?.result) {
    return { ok: false, error: { code: "bad-envelope", message: text.slice(0, 200), details: {} } };
  }
  return data.result as RpcResult<T>;
}

// ---------- wire → 前端消息模型 ----------

/** 把 wire 事件块集合成 text（assistant 多块拼接；tool-result 块忽略正文）。 */
function wireText(blocks: unknown[]): string {
  return (blocks as WireBlock[])
    .filter((b) => b?.type === "text" && typeof b.text === "string")
    .map((b) => b.text as string)
    .join("");
}

/** assistant/message 里的 reasoning 文本块（wire 上 thinking 以块形式出现）。 */
function wireThinking(blocks: unknown[]): string | undefined {
  const parts = (blocks as WireBlock[])
    .filter((b) => b?.type === "reasoning" && typeof b.text === "string")
    .map((b) => b.text as string);
  return parts.length ? parts.join("") : undefined;
}

export interface HistoryItem {
  event: WireEvent;
}

/** session.history → 前端消息数组（内部 turn 事件映射到推理卡；user/assistant 成对）。 */
export function historyToMessages(events: WireEvent[]): ChatMessage[] {
  const out: ChatMessage[] = [];
  const cur: ChatMessage = {
    id: uid("m"),
    role: "assistant",
    content: "",
    createdAt: Date.now(),
  };
  for (const ev of events) {
    const d = ev.data;
    switch (ev.type) {
      case "user/message": {
        const blocks = (d.content as unknown[]) ?? [];
        out.push({
          id: (d.id as string) ?? uid("m"),
          role: "user",
          content: wireText(blocks),
          createdAt: ev.time,
        });
        cur.content = "";
        cur.thinking = undefined;
        cur.tools = undefined;
        break;
      }
      case "assistant/chunk": {
        const chunk = d.chunk as { type?: string; text?: string } | undefined;
        const ctype = chunk?.type;
        const text = chunk?.text ?? "";
        if (ctype === "text-delta") {
          cur.content += text;
        } else if (ctype === "reasoning-delta") {
          cur.thinking = (cur.thinking ?? "") + text;
        } else if (ctype === "tool-call") {
          const tc = (chunk as unknown as Record<string, unknown>);
          const entry: ToolCall = {
            id: (tc.callId as string) ?? uid("t"),
            name: (tc.name as string) ?? "tool",
            summary: (tc.arguments as string)?.slice(0, 48) ?? "",
            status: "running",
          };
          if (cur.tools) {
            if (!cur.tools.find((t) => t.id === entry.id)) cur.tools.push(entry);
          } else {
            cur.tools = [entry];
          }
        }
        break;
      }
      case "assistant/message": {
        const msg = d.message as { id?: string; content?: unknown[]; role?: string } | undefined;
        if (!msg) break;
        const text = wireText(msg.content ?? []);
        const think = wireThinking(msg.content ?? []);
        const existing = out[out.length - 1];
        if (existing && existing.role === "assistant" && existing.createdAt === cur.createdAt) {
          if (text) existing.content = text;
          if (think !== undefined) existing.thinking = think;
        } else {
          cur.id = (msg.id as string) ?? uid("m");
          cur.content = text;
          if (think !== undefined) cur.thinking = think;
          out.push({ ...cur });
        }
        break;
      }
      case "tool/call": {
        const entry: ToolCall = {
          id: (d.callId as string) ?? uid("t"),
          name: (d.name as string) ?? "tool",
          summary: (d.arguments as string)?.slice(0, 48) ?? "",
          status: "running",
        };
        if (cur.tools) {
          if (!cur.tools.find((t) => t.id === entry.id)) cur.tools.push(entry);
        } else {
          cur.tools = [entry];
        }
        break;
      }
      case "tool/result": {
        const callId = (d.message as { source?: { callId?: string } })?.source?.callId;
        if (callId && cur.tools) {
          cur.tools = cur.tools.map((t) => (t.id === callId ? { ...t, status: "done" } : t));
        }
        break;
      }
      default:
        break;
    }
  }
  // 空助手消息（只有 reasoning/tool）也要呈现；彻底空内容丢弃。
  return out.filter((m) => m.content !== "" || m.thinking !== undefined || (m.tools?.length ?? 0) > 0);
}

export interface MuxFrame {
  type: "server-request";
  rpcId: string;
  method: string;
  payload: { type: string } & Record<string, unknown>;
}

/** 审批应答：POST /api/respond（client-response 信封；approval 先查的 pending 表）。
 * 回执 {accepted:true} 才算送达；失败保留审批项供重试。 */
export async function respondApproval(
  a: { rpcId: string; sessionId: string; approvalId: string },
  outcome: "allowed-once" | "rejected",
): Promise<boolean> {
  try {
    const res = await fetch("/api/respond", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        type: "client-response",
        rpcId: a.rpcId,
        result: { ok: true, value: { sessionId: a.sessionId, approvalId: a.approvalId, outcome } },
      }),
      signal: AbortSignal.timeout(RPC_TIMEOUT_MS),
    });
    const receipt = (await res.json()) as { accepted?: boolean };
    return Boolean(receipt?.accepted);
  } catch {
    return false;
  }
}

/** 打开下行 WS 流（mux 或 host；断线自动重连，指数退避 1s→15s）。onFrame(payload) 由调用方分发。 */
export function openMuxStream(
  onFrame: (frame: MuxFrame) => void,
  path = "/api/events.mux",
): { close: () => void } {
  let ws: WebSocket | null = null;
  // StrictMode 下 effect 会 mount→cleanup→mount 跑两次；closed 一旦置位就不该
  // 再生效（cleanup 只是断开当前重连循环，不是永久禁用）。用 generation 区分。
  let generation = 0;
  let timer: number | null = null;
  let attempt = 0;
  const connect = (gen: number) => {
    if (gen !== generation) return;
    try {
      ws = new WebSocket(`${location.protocol === "https:" ? "wss" : "ws"}://${location.host}${path}`);
    } catch {
      timer = window.setTimeout(() => connect(gen), backoff());
      return;
    }
    ws.onopen = () => {
      attempt = 0; // 连上即复位退避
    };
    ws.onmessage = (e: MessageEvent<string>) => {
      let frame: MuxFrame | null = null;
      try {
        frame = JSON.parse(e.data as string);
      } catch {
        return;
      }
      if (!frame || frame.type !== "server-request" || !frame.method) return;
      onFrame(frame);
    };
    ws.onclose = () => {
      if (gen !== generation) return;
      timer = window.setTimeout(() => connect(gen), backoff());
    };
    ws.onerror = () => {
      ws?.close();
    };
  };
  const backoff = () => Math.min(15000, 1000 * 2 ** attempt++);
  connect(generation);
  return {
    close() {
      // 递增 generation：使当前重连循环失效（包括正在排队的 setTimeout），
      // 并断开当前 socket。后续新 effect 重新调用会拿到新 generation。
      generation += 1;
      if (timer) window.clearTimeout(timer);
      ws?.close();
    },
  };
}