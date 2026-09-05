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
import { storage, STORAGE_KEYS, sessionsStore } from "@/lib/storage";

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

// 从 delta 中剥离审批标记;命中则回调 onApproval。
// W4b+ 加固:标记可能被代理/TCP 分包切开——发现 "[BM_APPROVAL:" 起暂存进
// 缓冲,直到闭合 "]"+换行才解析剥离;未决期间正文不透传标记碎片,
// 防止审批卡片丢失且裸 JSON 泄露进聊天气泡。
function createApprovalMarkerStream(
  onApproval: (req: ApprovalRequest) => void,
  pushText: (text: string) => void,
) {
  let buf = "";
  const flush = () => {
    if (buf) {
      pushText(buf);
      buf = "";
    }
  };
  const feed = (delta: string) => {
    let combined = buf + delta;
    buf = "";
    for (;;) {
      const start = combined.indexOf("[BM_APPROVAL:");
      if (start === -1) break;
      pushText(combined.slice(0, start));
      combined = combined.slice(start);
      const end = combined.indexOf("]\n", 1);
      if (end === -1) {
        // 标记未闭合:整段暂存,等下一个 delta 再续
        buf = combined;
        return;
      }
      const objStart = combined.indexOf("{");
      const jsonText = combined.slice(objStart === -1 ? 1 : objStart, end);
      try {
        const parsed = JSON.parse(jsonText) as {
          bm_approval_request?: Omit<ApprovalRequest, "status">;
        };
        if (parsed.bm_approval_request) {
          onApproval({ ...parsed.bm_approval_request, status: "waiting" });
        }
      } catch {
        // 完整闭合仍解析失败:按丢弃处理(不放裸 JSON 进正文)
        console.warn("[BM_APPROVAL] 标记解析失败,已丢弃");
      }
      combined = combined.slice(end + 2);
    }
    pushText(combined);
  };
  return { feed, flush };
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
    const pushText = (text: string) => {
      if (!text) return;
      setMessages((cur) => {
        if (cur.length === 0) return cur;
        const copy = [...cur];
        const last = copy[copy.length - 1];
        const parts = (last.content as TextPart[]).map((p) => ({ ...p }));
        parts[0] = { ...parts[0], text: parts[0].text + text };
        copy[copy.length - 1] = { ...last, content: parts };
        return copy;
      });
    };
    // W4b+:审批标记流式缓冲(跨 chunk 粘包/分包安全)
    const markerStream = createApprovalMarkerStream(
      (req) => approvalHandlerRef.current(req),
      pushText,
    );
    const appendDelta = (delta: string) => markerStream.feed(delta);
    approvalHandlerRef.current = (req) => {
      const permMode = storage.get(STORAGE_KEYS.PERMISSION_MODE) || "ask";
      // 完全访问 (YOLO 模式): 自动放行批准，界面不弹卡片或抽屉
      if (permMode === "yolo") {
        void fetch(
          `/admin/approvals/${encodeURIComponent(req.approval_id)}/respond`,
          {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              decision: "approve",
              scope: "once",
            }),
          },
        ).catch(() => {});
        return;
      }
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
            // W8:随消息携带所选工作目录(空 = 不绑定,服务器默认)
            ...(storage.get(STORAGE_KEYS.ACTIVE_WORKSPACE)
              ? { workspace: storage.get(STORAGE_KEYS.ACTIVE_WORKSPACE) }
              : {}),
            stream: true,
            messages: [{ role: "user", content: text }],
          }),
          signal: controller.signal,
        });
      };
      let res = await doFetch(true);
      if (res.status === 400) {
        const detail = await res.text().catch(() => "");
        // W8:所选工作区未登记/已删除 → 清本地选择并给出可读提示
        // (不与「未知会话」重试混淆:换会话救不了坏工作区)
        if (detail.includes("工作区")) {
          storage.remove(STORAGE_KEYS.ACTIVE_WORKSPACE);
          window.dispatchEvent(new CustomEvent("bm-workspaces-changed"));
          throw new Error(
            "所选工作目录不可用(可能已被删除):请重新选择,或到 设置→常规 检查",
          );
        }
        // 服务器重启会清空内存会话表:400「未知会话」→ 清记忆重开新会话重试一次
        storage.remove(STORAGE_KEYS.SESSION);
        res = await doFetch(false);
      }
      if (!res.ok || !res.body) {
        const detail = await res.text().catch(() => "");
        throw new Error(`HTTP ${res.status} ${detail.slice(0, 160)}`);
      }
      const newSid = res.headers.get("x-bm-session");
      if (newSid) {
        storage.set(STORAGE_KEYS.SESSION, newSid);
        const title = text.slice(0, 24) || "新对话";
        sessionsStore.upsert(newSid, title);
        window.dispatchEvent(new CustomEvent("bm-sessions-updated"));
      }

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
      // 流正常收尾:冲刷可能残留的未闭合标记缓冲(按原样上屏,不吞正文)
      markerStream.flush();
    } catch (e) {
      // 异常收尾:同样先冲刷缓冲再追加错误提示
      markerStream.flush();
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
  // ?mock_approval=1: 方便即时在浏览器中直观预览和调试审批抽屉悬浮条效果
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    if (params.get("mock_approval") === "1") {
      setMessages([
        {
          role: "user",
          content: [{ type: "text", text: "请帮我检查项目根目录并编译构建" }],
        },
        {
          role: "assistant",
          content: [
            {
              type: "text",
              text: "好的，我需要先执行系统命令列出项目目录下的文件结构，以确认构建工具和配置环境：",
            },
          ],
        },
      ]);
      setPendingApprovals([
        {
          approval_id: "appr_01MOCK999888",
          capability: "system.exec",
          args: {
            command: "cargo build --release --workspace",
            cwd: "D:\\96_CoderWorld\\BoenMind",
          },
          operation_id: "op_mock_123",
          status: "waiting",
        },
      ]);
      return;
    }

    const q = params.get("e2e");
    if (!q) return;
    const t = setTimeout(() => sendUserText(q), 800);
    return () => clearTimeout(t);
  }, []);

  // 「新建对话」(SessionPanel 加号派发 bm-chat-new):中止在途回合、
  // 丢弃会话号、清空消息视图与审批挂起;下一条消息即自动开新会话。appendDelta
  // 对空消息列表是安全空操作,中止回调不会把内容写回已清空的视图。
  useEffect(() => {
    const onNewChat = () => {
      abortRef.current?.abort();
      setIsRunning(false);
      storage.remove(STORAGE_KEYS.SESSION);
      setMessages([]);
      setPendingApprovals([]);
    };
    window.addEventListener("bm-chat-new", onNewChat);
    return () => window.removeEventListener("bm-chat-new", onNewChat);
  }, []);

  // 「切换历史会话」(SessionPanel 派发 bm-session-switched,目标 sid 已由
  // App.tsx 写入 storage):中止在途回合并复位消息视图与审批挂起。
  // 历史消息回放需独立端点(未建,见 BACKLOG),复位保证至少不再串显旧会话内容。
  useEffect(() => {
    const onSessionSwitched = () => {
      abortRef.current?.abort();
      setIsRunning(false);
      setMessages([]);
      setPendingApprovals([]);
    };
    window.addEventListener("bm-session-switched", onSessionSwitched);
    return () =>
      window.removeEventListener("bm-session-switched", onSessionSwitched);
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
    // 乐观从等待队列移除，悬浮抽屉即刻收起
    setPendingApprovals((cur) =>
      cur.filter((a) => a.approval_id !== approvalId),
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
      // 裁决失败保持原样
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
