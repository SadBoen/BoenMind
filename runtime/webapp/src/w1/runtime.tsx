// W1(ADR-0014):ExternalStore 运行时接线——壳子状态 ↔ /v1/chat/completions
// 合同:见 milestones/W1-implementation-spec.md §4/§5
// W4b:对话内审批——SSE delta 中的 [BM_APPROVAL:{...}] 标记不入正文,
// 转入 pendingApprovals 状态,由 thread.tsx 渲染审批卡片。
import {
  AssistantRuntimeProvider,
  useExternalStoreRuntime,
} from "@assistant-ui/react";
import type { AppendMessage, ThreadMessageLike } from "@assistant-ui/react";
import { createContext, useContext, useEffect, useRef, useState } from "react";
import { storage, STORAGE_KEYS } from "@/lib/storage";

type TextPart = { type: "text"; text: string };

export type ApprovalRequest = {
  approval_id: string;
  capability: string;
  args: unknown;
  operation_id: string;
  status: "waiting" | "approved" | "denied";
};

// W4b:审批卡片状态与裁决动作,经 context 提供给 thread.tsx
type ApprovalContextValue = {
  pendingApprovals: ApprovalRequest[];
  respondApproval: (id: string, decision: "approve" | "deny") => Promise<void>;
};
const BoenmindRuntimeContext = createContext<ApprovalContextValue>({
  pendingApprovals: [],
  respondApproval: async () => {},
});
export const useBoenmindApprovals = () => useContext(BoenmindRuntimeContext);

// 从 delta 中剥离审批标记;命中则回调 onApproval
function extractApprovalMarker(
  delta: string,
  onApproval: (req: ApprovalRequest) => void,
): string {
  const start = delta.indexOf("[BM_APPROVAL:");
  if (start === -1) return delta;
  const before = delta.slice(0, start);
  const objStart = delta.indexOf("{", start);
  if (objStart === -1) return before;
  // 找配对的右括号(标记为单行 JSON,取最后一个 } 直到 ])
  const end = delta.indexOf("]\n", objStart);
  const jsonText = end === -1 ? delta.slice(objStart) : delta.slice(objStart, end);
  try {
    const parsed = JSON.parse(jsonText.replace(/\]\s*$/, "")) as {
      bm_approval_request?: {
        approval_id: string;
        capability: string;
        args: unknown;
        operation_id: string;
      };
    };
    if (parsed.bm_approval_request) {
      onApproval({
        ...parsed.bm_approval_request,
        status: "waiting",
      });
    }
  } catch {
    // 标记不完整(分块到达):忽略,后续完整块再解析
  }
  return before;
}

export function BoenmindRuntimeProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const [messages, setMessages] = useState<ThreadMessageLike[]>([]);
  const [isRunning, setIsRunning] = useState(false);
  const [pendingApprovals, setPendingApprovals] = useState<ApprovalRequest[]>(
    [],
  );
  // 生成中可随时中止(点「停止」):中断 SSE 并立即解锁输入框;
  // 服务器侧该回合仍会后台完成并落库(W1 口径,不丢)
  const abortRef = useRef<AbortController | null>(null);
  const approvalHandlerRef = useRef<(req: ApprovalRequest) => void>(() => {});

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
      const cleaned = extractApprovalMarker(
        delta,
        (req) => approvalHandlerRef.current(req),
      );
      if (!cleaned) return;
      setMessages((cur) => {
        if (cur.length === 0) return cur;
        const copy = [...cur];
        const last = copy[copy.length - 1];
        const parts = (last.content as TextPart[]).map((p) => ({ ...p }));
        parts[0] = { ...parts[0], text: parts[0].text + cleaned };
        copy[copy.length - 1] = { ...last, content: parts };
        return copy;
      });
    };
    approvalHandlerRef.current = (req) => {
      setPendingApprovals((cur) => {
        if (cur.some((a) => a.approval_id === req.approval_id)) return cur;
        return [...cur, req];
      });
    };

    const controller = new AbortController();
    abortRef.current = controller;
    // 壳侧看门狗:60 秒无任何增量/完成即中止解锁(服务器侧硬上限 180s)
    let watchdog = setTimeout(() => controller.abort(), 60_000);
    const poke = () => {
      clearTimeout(watchdog);
      watchdog = setTimeout(() => controller.abort(), 60_000);
    };

    try {
      const doFetch = (withSession: boolean) => {
        const headers: Record<string, string> = {
          "Content-Type": "application/json",
        };
        const sid = withSession ? storage.get(STORAGE_KEYS.SESSION) : null;
        if (sid) headers["X-Bm-Session"] = sid;
        const roleId = storage.get(STORAGE_KEYS.ACTIVE_ROLE);
        if (roleId) headers["X-Bm-Role"] = roleId;
        return fetch("/v1/chat/completions", {
          method: "POST",
          headers,
          body: JSON.stringify({
            // W6:随消息携带所选模型(localStorage 持久化;空 = "auto" = 服务器默认)
            model: storage.get(STORAGE_KEYS.ACTIVE_MODEL) || "auto",
            stream: true,
            messages: [{ role: "user", content: text }],
          }),
          signal: controller.signal,
        });
      };
      let res = await doFetch(true);
      // 服务器重启会清空内存会话表:400「未知会话」→ 清记忆重开新会话重试一次
      if (res.status === 400) {
        storage.remove(STORAGE_KEYS.SESSION);
        res = await doFetch(false);
      }
      if (!res.ok || !res.body) {
        const detail = await res.text().catch(() => "");
        throw new Error(`HTTP ${res.status} ${detail.slice(0, 160)}`);
      }
      const newSid = res.headers.get("x-bm-session");
      if (newSid) storage.set(STORAGE_KEYS.SESSION, newSid);

      const reader = res.body.getReader();
      const dec = new TextDecoder();
      let buf = "";
      stream: while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        poke();
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
      const aborted = e instanceof DOMException && e.name === "AbortError";
      appendDelta(
        aborted
          ? "\n[已停止]"
          : `\n[连接失败: ${e instanceof Error ? e.message : String(e)}]`,
      );
    } finally {
      clearTimeout(watchdog);
      abortRef.current = null;
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

  // 「新建对话」(SessionPanel 加号派发 bm-chat-new):中止在途回合、
  // 丢弃会话号、清空消息视图;下一条消息即自动开新会话。appendDelta
  // 对空消息列表是安全空操作,中止回调不会把内容写回已清空的视图。
  useEffect(() => {
    const onNewChat = () => {
      abortRef.current?.abort();
      storage.remove(STORAGE_KEYS.SESSION);
      setMessages([]);
    };
    window.addEventListener("bm-chat-new", onNewChat);
    return () => window.removeEventListener("bm-chat-new", onNewChat);
  }, []);

  useEffect(() => {
    document.title = "BM n=" + messages.length + " run=" + isRunning;
  }, [messages, isRunning]);

  // W4b:审批裁决(前端卡片按钮)→ /admin/approvals/{id}/respond
  // (与 /rpc 同一执行体,走 /admin 免鉴权口径——前端无令牌可带)
  const respondApproval = async (
    approvalId: string,
    decision: "approve" | "deny",
  ) => {
    setPendingApprovals((cur) =>
      cur.map((a) =>
        a.approval_id === approvalId
          ? { ...a, status: decision === "approve" ? "approved" : "denied" }
          : a,
      ),
    );
    try {
      await fetch(
        `/admin/approvals/${encodeURIComponent(approvalId)}/respond`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            decision,
            scope: decision === "approve" ? "once" : undefined,
          }),
        },
      );
    } catch {
      // 裁决失败保持卡片状态,可重试(卡片仍显示)
    }
  };

  const runtime = useExternalStoreRuntime({
    messages,
    setMessages: (m) => setMessages([...m]),
    onNew,
    isRunning,
    convertMessage: (m) => m,
    onCancel: async () => {
      abortRef.current?.abort();
    },
  });

  return (
    <BoenmindRuntimeContext.Provider
      value={{ pendingApprovals, respondApproval }}
    >
      <AssistantRuntimeProvider runtime={runtime}>
        {children}
      </AssistantRuntimeProvider>
    </BoenmindRuntimeContext.Provider>
  );
}
