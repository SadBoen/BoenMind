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

// ---- 文件面（工作目录作用域）----

/** workdir 文件下载 URL（<img src> 直接内嵌预览；同源 cookie 自动携带） */
export function downloadUrl(path: string): string {
  return `/api/host.download?path=${encodeURIComponent(path)}`;
}

/** 触发浏览器下载（a[download] + blob） */
export async function downloadFile(path: string, name?: string): Promise<void> {
  const res = await fetch(downloadUrl(path));
  if (!res.ok) throw new Error(`下载失败 (${res.status})`);
  const blob = await res.blob();
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name || path.split("/").pop() || "file";
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

/** 上传文件到 workdir 内目录（multipart：dir 相对路径 + file）。overwrite=true 覆盖同名。 */
export async function uploadFile(
  dir: string,
  file: File,
  overwrite = false,
): Promise<void> {
  const form = new FormData();
  form.append("dir", dir);
  form.append("file", file, file.name);
  const res = await fetch("/api/host.upload", {
    method: "POST",
    headers: overwrite ? { "x-bm-overwrite": "true" } : undefined,
    body: form,
  });
  if (res.status === 401 || res.status === 403) throw new AuthRequiredError();
  if (res.status === 409) throw new Error("同名文件已存在（如需覆盖请开启覆盖选项）");
  if (res.status === 413) throw new Error("文件超过大小上限");
  if (!res.ok) throw new Error(`上传失败 (${res.status})`);
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
