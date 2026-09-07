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
import { BM_EVENTS, emit } from "../lib/bus";
import { api } from "../w2/api";

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
  // 编辑历史消息并从该处重新生成（分支功能）
  editAndBranchMessage: (messageIndex: number, newText: string) => Promise<void>;
  // 重新生成最后一条回复（分支功能）
  regenerateMessage: (messageIndex: number) => Promise<void>;
  // 会话历史分页(2026-09-06):「加载更早消息」由 thread.tsx 顶部按钮触发
  history: {
    hasMore: boolean;
    loading: boolean;
    loadOlder: () => void;
  };
};
const BoenmindRuntimeContext = createContext<ApprovalContextValue>({
  pendingApprovals: [],
  respondApproval: async () => {},
  editAndBranchMessage: async () => {},
  regenerateMessage: async () => {},
  history: { hasMore: false, loading: false, loadOlder: () => {} },
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

// 回放消息 → 线程消息形状(2026-09-06;切会话/刷新/加载更早三路共用)
function toThreadMessages(
  rows: { seq: number | null; role: "user" | "assistant"; content: string }[],
): ThreadMessageLike[] {
  return rows.map((m) => ({
    role: m.role,
    content: [{ type: "text", text: m.content }] as TextPart[],
  }));
}

export function BoenmindRuntimeProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const [messages, setMessages] = useState<ThreadMessageLike[]>([]);
  const [isRunning, setIsRunning] = useState(false);
  // 历史回放分页:已加载的历史条数(= 从末尾 skip 的游标)+ 是否还有更早
  // (每页 50,「加载更早」增量前插,防长会话一口气载入卡界面;游标不用
  // seq——历史文件 seq 跨重启重数)
  const HISTORY_PAGE = 50;
  const historyCountRef = useRef(0);
  const [historyMore, setHistoryMore] = useState({ hasMore: false, loading: false });
  const [pendingApprovals, setPendingApprovals] = useState<ApprovalRequest[]>(
    [],
  );
  // 生成中可随时中止(点「停止」):中断 SSE 并立即解锁输入框;
  // 服务器侧该回合仍会后台完成并落库(W1 口径,不丢)
  const abortRef = useRef<AbortController | null>(null);
  const approvalHandlerRef = useRef<(req: ApprovalRequest) => void>(() => {});
  // 批准可达性轮询去重集(2026-09-07):已处理过(已批准/已入抽屉)的审批 id
  // (P1-26 收口:流内标记入抽屉时同样登记,轮询不再重复入队)
  const handledApprovalsRef = useRef<Set<string>>(new Set());
  // 会话视图代(P1-28):切会话/加载更早的自回放请求带代数,在途响应返回时
  // 代数不符即丢弃——旧会话迟到响应不再前插到新会话消息上
  const sessionEpochRef = useRef(0);

  // 批准可达性轮询(2026-09-07 审批卡死根治):审批标记仅随回合 /v1 流下发,
  // 流到期或后台续跑回合无主流时,审批单永远无人可批,任务卡死在审批轮询。
  // 此通道每 2.5s 拉一次待裁决队列:YOLO 自动批准,ask 进抽屉(与流内标记
  // 按 approval_id 去重,双通道互为兜底)。
  useEffect(() => {
    const tick = async () => {
      try {
        const res = await fetch("/admin/approvals");
        if (!res.ok) return;
        const data = (await res.json()) as {
          approvals?: Array<{
            approval_id: string;
            capability?: string;
            args_summary?: string;
          }>;
        };
        for (const a of data.approvals ?? []) {
          if (handledApprovalsRef.current.has(a.approval_id)) continue;
          handledApprovalsRef.current.add(a.approval_id);
          const permMode = storage.get(STORAGE_KEYS.PERMISSION_MODE) || "ask";
          if (permMode === "yolo") {
            const post = fetch(
              `/admin/approvals/${encodeURIComponent(a.approval_id)}/respond`,
              {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ decision: "approve", scope: "once" }),
              },
            );
            void post
              .then((r) => {
                // 批准失败(过期/非等待态)不重试;网络抖动则下一轮询重试
                if (!r.ok && r.status >= 500)
                  handledApprovalsRef.current.delete(a.approval_id);
              })
              .catch(() => handledApprovalsRef.current.delete(a.approval_id));
          } else {
            setPendingApprovals((cur) =>
              cur.some((p) => p.approval_id === a.approval_id)
                ? cur
                : [
                    ...cur,
                    {
                      approval_id: a.approval_id,
                      capability: a.capability ?? "unknown",
                      args: { summary: a.args_summary ?? "" },
                      operation_id: "",
                      status: "waiting" as const,
                    },
                  ],
            );
          }
        }
      } catch {
        // 服务未起/重启窗口:静默,下一 tick 重试
      }
    };
    const iv = setInterval(tick, 2500);
    return () => clearInterval(iv);
  }, []);

  // 审批裁决 POST 公共实现(P1-2/P1-26 收口):检查 res.ok、失败回滚入队
  // 并从去重集摘除(下一轮询兜底重试),不再静默吞错
  const postApprovalRespond = async (
    approvalId: string,
    decision: "approve" | "deny",
  ): Promise<boolean> => {
    try {
      const r = await fetch(
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
      if (!r.ok) {
        console.warn(
          `审批裁决失败(HTTP ${r.status}): ${approvalId} ${decision}`,
        );
        return false;
      }
      return true;
    } catch (e) {
      console.warn(`审批裁决请求失败: ${approvalId}`, e);
      return false;
    }
  };

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
      // P1-26:流内到达即登记去重集,轮询通道不再重复入队
      handledApprovalsRef.current.add(req.approval_id);
      const permMode = storage.get(STORAGE_KEYS.PERMISSION_MODE) || "ask";
      // 完全访问 (YOLO 模式): 自动放行批准，界面不弹卡片或抽屉
      if (permMode === "yolo") {
        void postApprovalRespond(req.approval_id, "approve").then((ok) => {
          // 失败从去重集摘除:轮询通道下一 tick 兜底重试
          if (!ok) handledApprovalsRef.current.delete(req.approval_id);
        });
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
      // 门户会话失效(2026-09-06):正向跳登录,不再让用户对着红条猜
      if (res.status === 401) {
        window.location.href = "/login";
        throw new Error("需要登录");
      }
      if (res.status === 400) {
        const detail = await res.text().catch(() => "");
        // W8:所选工作区未登记/已删除 → 清本地选择并给出可读提示
        // (不与「未知会话」重试混淆:换会话救不了坏工作区)
        if (detail.includes("工作区")) {
          storage.remove(STORAGE_KEYS.ACTIVE_WORKSPACE);
          emit(BM_EVENTS.workspacesChanged);
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
        emit(BM_EVENTS.sessionsUpdated);
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
            error?: { message?: string; code?: string };
          };
          const d = v.choices?.[0]?.delta?.content;
          if (typeof d === "string" && d) appendDelta(d);
          // P1-11 配套(2026-09-07 架构评审):服务器失败/超时/中断不再谎报
          // finish stop+[DONE],改发 OpenAI 兼容错误帧——上屏告知用户
          else if (v.error?.message) {
            appendDelta(`\n[流式错误: ${v.error.message}]`);
          }
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
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
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
      sessionEpochRef.current += 1; // P1-28:作废在途回放响应
      setMessages([]);
      setPendingApprovals([]);
    };
    window.addEventListener(BM_EVENTS.chatNew, onNewChat);
    return () => window.removeEventListener(BM_EVENTS.chatNew, onNewChat);
  }, []);

  // 「切换历史会话」(SessionPanel 派发 bm-session-switched,目标 sid 已由
  // App.tsx 写入 storage):中止在途回合,拉取该会话历史消息回放(2026-09-06
  // 落地,原 BACKLOG「会话历史回放端点」);拉取失败保持空视图不再串显。
  // P1-28:自增会话视图代,在途回放响应返回时代数不符即丢弃。
  useEffect(() => {
    const onSessionSwitched = async () => {
      abortRef.current?.abort();
      setIsRunning(false);
      setMessages([]);
      setPendingApprovals([]);
      const epoch = ++sessionEpochRef.current;
      const sid = storage.get(STORAGE_KEYS.SESSION);
      if (!sid) return;
      try {
        const res = await api.sessionMessages(sid, { limit: HISTORY_PAGE });
        if (sessionEpochRef.current !== epoch) return; // 已切走,丢弃迟到响应
        setMessages(toThreadMessages(res.messages ?? []));
        historyCountRef.current = (res.messages ?? []).length;
        setHistoryMore({ hasMore: res.has_more ?? false, loading: false });
      } catch {
        // 回放失败(日志缺失/网络抖动)保持空视图,不打断用户输入
      }
    };
    window.addEventListener(BM_EVENTS.sessionSwitched, onSessionSwitched);
    return () =>
      window.removeEventListener(BM_EVENTS.sessionSwitched, onSessionSwitched);
  }, []);

  // 页面刷新后:本地仍记着会话 id 时同样回放历史(否则刷新即空白)
  useEffect(() => {
    const sid = storage.get(STORAGE_KEYS.SESSION);
    if (!sid) return;
    // P1-28:与切会话共用视图代——刷新回放在途时用户切走,响应作废
    const epoch = sessionEpochRef.current;
    let cancelled = false;
    void api
      .sessionMessages(sid, { limit: HISTORY_PAGE })
      .then((res) => {
        if (cancelled || sessionEpochRef.current !== epoch) return;
        setMessages(toThreadMessages(res.messages ?? []));
        historyCountRef.current = (res.messages ?? []).length;
        setHistoryMore({ hasMore: res.has_more ?? false, loading: false });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
    // 仅挂载时执行一次
  }, []);

  useEffect(() => {
    document.title = "BM n=" + messages.length + " run=" + isRunning;
  }, [messages, isRunning]);

  // W4b:审批裁决(前端卡片按钮)→ /admin/approvals/{id}/respond
  // (与 /rpc 同一执行体,走 /admin 免鉴权口径——前端无令牌可带)
  // P1-2(2026-09-07 架构评审):检查 res.ok;失败把审批单放回抽屉
  // (乐观移除回滚)并从去重集摘除,不再静默吞错让审批单凭空消失
  const respondApproval = async (
    approvalId: string,
    decision: "approve" | "deny",
  ) => {
    // 先留底(失败回滚时原样放回 capability/args)
    const original = pendingApprovals.find((a) => a.approval_id === approvalId);
    // 乐观从等待队列移除，悬浮抽屉即刻收起
    setPendingApprovals((cur) =>
      cur.filter((a) => a.approval_id !== approvalId),
    );
    const ok = await postApprovalRespond(approvalId, decision);
    if (!ok) {
      // 回滚:重新入抽屉等待用户重试(服务端已翻单则 400,重试无害)
      handledApprovalsRef.current.delete(approvalId);
      setPendingApprovals((cur) =>
        cur.some((a) => a.approval_id === approvalId)
          ? cur
          : [
              ...cur,
              original ?? {
                approval_id: approvalId,
                capability: "unknown",
                args: {},
                operation_id: "",
                status: "waiting" as const,
              },
            ],
      );
    }
  };

  // 编辑历史消息并从该点开辟新分支
  const editAndBranchMessage = async (messageIndex: number, newText: string) => {
    if (isRunning) {
      abortRef.current?.abort();
      setIsRunning(false);
    }
    // 截断该消息之后的所有消息，并更新当前消息文本
    setMessages((cur) => {
      const truncated = cur.slice(0, messageIndex);
      return truncated;
    });
    // 以新文本重新发送
    await sendUserText(newText);
  };

  // 重新生成指定消息之后的内容（分支）
  const regenerateMessage = async (messageIndex: number) => {
    if (isRunning) {
      abortRef.current?.abort();
      setIsRunning(false);
    }
    // 找到上一条用户消息
    let userText = "";
    setMessages((cur) => {
      const target = cur[messageIndex];
      if (target?.role === "user") {
        userText = (target.content[0] as TextPart)?.text || "";
        return cur.slice(0, messageIndex);
      } else {
        const prevUser = cur.slice(0, messageIndex).reverse().find((m) => m.role === "user");
        userText = (prevUser?.content[0] as TextPart)?.text || "";
        const prevIndex = cur.slice(0, messageIndex).lastIndexOf(prevUser!);
        return prevIndex >= 0 ? cur.slice(0, prevIndex) : cur.slice(0, messageIndex);
      }
    });
    if (userText) {
      await sendUserText(userText);
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

  // 「加载更早消息」:skip=已加载条数,取前一页并前插(2026-09-06)
  // P1-28:在途期间切会话则丢弃响应(视图代守卫),不再前插到新会话上
  const loadOlder = async () => {
    const sid = storage.get(STORAGE_KEYS.SESSION);
    if (!sid || historyMore.loading) return;
    const epoch = sessionEpochRef.current;
    setHistoryMore((h) => ({ ...h, loading: true }));
    try {
      const res = await api.sessionMessages(sid, {
        limit: HISTORY_PAGE,
        skip: historyCountRef.current,
      });
      if (sessionEpochRef.current !== epoch) return; // 已切走,丢弃
      const older = toThreadMessages(res.messages ?? []);
      setMessages((cur) => [...older, ...cur]);
      historyCountRef.current += older.length;
      setHistoryMore({ hasMore: res.has_more ?? false, loading: false });
    } catch {
      setHistoryMore((h) => ({ ...h, loading: false }));
    }
  };

  return (
    <BoenmindRuntimeContext.Provider
      value={{
        pendingApprovals,
        respondApproval,
        editAndBranchMessage,
        regenerateMessage,
        history: {
          hasMore: historyMore.hasMore,
          loading: historyMore.loading,
          loadOlder: () => void loadOlder(),
        },
      }}
    >
      <AssistantRuntimeProvider runtime={runtime}>
        {children}
      </AssistantRuntimeProvider>
    </BoenmindRuntimeContext.Provider>
  );
}
