// 后端 RPC 客户端：Rust 内核协议兼容层（web-server）的 JSON 信封。
// 认证：优先 localStorage token → x-boenmind-session 头（后端也认 Cookie）。

const TOKEN_KEY = "bm_session_token";

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}
export function setToken(t: string | null) {
  if (t) localStorage.setItem(TOKEN_KEY, t);
  else localStorage.removeItem(TOKEN_KEY);
}

export interface RpcEnvelope<T = unknown> {
  type: "server-response";
  rpcId: string;
  result: { ok: true; value: T } | { ok: false; error: { code: string; message: string } };
}

export async function rpc<T = unknown>(method: string, payload: unknown = {}): Promise<T> {
  const res = await fetch(`/api/${method}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(getToken() ? { "x-boenmind-session": getToken()! } : {}),
    },
    body: JSON.stringify({ type: "client-request", rpcId: `r${Date.now()}`, method, payload }),
  });
  if (res.status === 401 || res.status === 403) {
    throw new AuthRequiredError();
  }
  const env = (await res.json()) as RpcEnvelope<T>;
  if (!env.result.ok) {
    const err = env.result.error;
    if (err.code === "auth-required") throw new AuthRequiredError();
    throw new Error(err.message || err.code);
  }
  return env.result.value;
}

export class AuthRequiredError extends Error {
  constructor() {
    super("auth required");
    this.name = "AuthRequiredError";
  }
}

// ---- 类型（与 kernel-contracts wire 形状对齐）----

export interface SessionSummary {
  sessionId: string;
  blank: boolean;
  running: boolean;
  cwd?: string;
  updatedAt?: string;
}

export interface ContentBlock {
  type?: string;
  text?: string;
  reasoning?: string;
  toolCall?: unknown;
  toolResult?: unknown;
}

// WS 下行事件（SessionRecord wire 形状）
export type WsEvent =
  | { seq: number; session_id: string; event: { type: "SessionStarted"; header?: unknown } }
  | { seq: number; session_id: string; event: { type: "UserMessage"; text: string } }
  | { seq: number; session_id: string; event: { type: "AssistantChunk"; chunk: unknown } }
  | { seq: number; session_id: string; event: { type: "AssistantMessage"; content: ContentBlock[]; usage?: unknown } }
  | { seq: number; session_id: string; event: { type: string; [k: string]: unknown } };
