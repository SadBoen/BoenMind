// W1(ADR-0014):ExternalStore 运行时接线——壳子状态 ↔ /v1/chat/completions
// 合同:见 milestones/W1-implementation-spec.md §4/§5
// W4b:对话内审批——SSE delta 中的 [BM_APPROVAL:{...}] 标记不入正文,
// 转入 pendingApprovals 状态,由 thread.tsx 渲染审批卡片。
import {
  AssistantRuntimeProvider,
  useExternalStoreRuntime,
} from "@assistant-ui/react";
import type { AppendMessage, ThreadMessageLike } from "@assistant-ui/react";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { storage, STORAGE_KEYS } from "@/lib/storage";
import { redirectToLogin } from "@/lib/utils";
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

// ADR-0055:工具调用事件(由后端 /v1 流的 bm_event 结构化帧直送,不再解析
// 模型正文文本标记)。effect 为后端 manifest 声明的风险等级——前端据此分类,
// 不再按工具名子串猜。
export type ToolEvent = {
  operationId: string;
  capability: string;
  effect: string;
  target: string;
  status: "running" | "done";
  elapsedMs?: number;
};

// bm_event 帧的解析结果(与 openai_compat.rs bm_event_chunk 同形)。
type BmEvent = { type: string; payload: Record<string, unknown> };

// W4b:审批卡片状态与裁决动作,经 context 提供给 thread.tsx
type ApprovalContextValue = {
  pendingApprovals: ApprovalRequest[];
  respondApproval: (id: string, decision: "approve" | "deny") => Promise<void>;
  // ADR-0055:本回合工具调用事件(结构化,替代正文标记解析);供助手消息渲染工具卡
  toolEvents: ToolEvent[];
  // 编辑历史消息并从该处重新生成（分支功能）
  editAndBranchMessage: (messageIndex: number, newText: string) => Promise<void>;
  // 重新生成最后一条回复（分支功能）
  regenerateMessage: (messageIndex: number) => Promise<void>;
  // 会话历史分页:「加载更早消息」由 thread.tsx 顶部按钮触发
  history: {
    hasMore: boolean;
    loading: boolean;
    loadOlder: () => void;
    // #27 窗口化渲染:DOM 只挂最近 windowSize 条;expandOlder 即时扩窗(不取数)
    windowSize: number;
    expandOlder: () => void;
    // 已加载条数达 HISTORY_TOTAL_CAP,不再向服务端取更早
    capped: boolean;
  };
};
const BoenmindRuntimeContext = createContext<ApprovalContextValue>({
  pendingApprovals: [],
  respondApproval: async () => {},
  toolEvents: [],
  editAndBranchMessage: async () => {},
  regenerateMessage: async () => {},
  history: {
    hasMore: false,
    loading: false,
    loadOlder: () => {},
    windowSize: 0,
    expandOlder: () => {},
    capped: false,
  },
});
export const useBoenmindApprovals = () => useContext(BoenmindRuntimeContext);

// ADR-0055:把后端 bm_event 结构化帧(工具调用/审批)分派到对应回调。
// 后端已不再把 UI 信息塞进模型正文,故前端无需任何文本标记解析/缓冲——
// 工具分类据 effect 字段,审批据 approval.requested 事件,各自结构清晰。
export function handleBmEvent(
  ev: BmEvent,
  handlers: {
    onApproval: (req: ApprovalRequest) => void;
    onToolStarted: (t: ToolEvent) => void;
    onToolInvoked: (operationId: string, effect: string) => void;
  },
) {
  const p = ev.payload ?? {};
  if (ev.type === "capability.started") {
    handlers.onToolStarted({
      operationId: String(p.operation_id ?? ""),
      capability: String(p.capability ?? ""),
      effect: String(p.effect ?? ""),
      target: String(p.target ?? ""),
      status: "running",
    });
  } else if (ev.type === "capability.invoked") {
    handlers.onToolInvoked(
      String(p.operation_id ?? ""),
      String(p.effect ?? ""),
    );
  } else if (ev.type === "approval.requested") {
    handlers.onApproval({
      approval_id: String(p.approval_id ?? ""),
      capability: String(p.capability ?? ""),
      args: p.args ?? null,
      operation_id: String(p.operation_id ?? ""),
      status: "waiting",
    });
  }
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
  // #27 长会话窗口化:DOM 只挂最近 MSG_WINDOW_DEFAULT 条;扩窗步进
  // MSG_WINDOW_STEP;累计加载上限 HISTORY_TOTAL_CAP(防无限堆积拖爆内存)
  const MSG_WINDOW_DEFAULT = 120;
  const MSG_WINDOW_STEP = 200;
  const HISTORY_TOTAL_CAP = 1000;
  const historyCountRef = useRef(0);
  const [historyMore, setHistoryMore] = useState({
    hasMore: false,
    loading: false,
    capped: false,
  });
  const [msgWindow, setMsgWindow] = useState(MSG_WINDOW_DEFAULT);
  const [pendingApprovals, setPendingApprovals] = useState<ApprovalRequest[]>(
    [],
  );
  // ADR-0055:本回合工具调用事件(结构化);每回合清空,随 bm_event 帧累积。
  const [toolEvents, setToolEvents] = useState<ToolEvent[]>([]);
  // 生成中可随时中止(点「停止」):中断 SSE 并立即解锁输入框;
  // 服务器侧该回合仍会后台完成并落库(W1 口径,不丢)
  const abortRef = useRef<AbortController | null>(null);
  const approvalHandlerRef = useRef<(req: ApprovalRequest) => void>(() => {});
  // 批准可达性轮询去重集:已处理过(已批准/已入抽屉)的审批 id
  // (P1-26 收口:流内标记入抽屉时同样登记,轮询不再重复入队)
  const handledApprovalsRef = useRef<Set<string>>(new Set());
  // 会话视图代(P1-28):切会话/加载更早的自回放请求带代数,在途响应返回时
  // 代数不符即丢弃——旧会话迟到响应不再前插到新会话消息上
  const sessionEpochRef = useRef(0);

  // #25:审批无人在线提醒——页面隐藏时浏览器通知 + 标题闪烁,回到前台即停。
  // 通知权限惰性请求(首个隐藏期审批触发);双通道(流内/轮询)入队点共用。
  const titleFlashRef = useRef<number | null>(null);
  const stopTitleFlash = useCallback(() => {
    if (titleFlashRef.current !== null) {
      window.clearInterval(titleFlashRef.current);
      titleFlashRef.current = null;
      document.title = "BoenMind";
    }
  }, []);
  const startTitleFlash = useCallback(() => {
    if (titleFlashRef.current !== null) return;
    const orig = document.title;
    let on = false;
    titleFlashRef.current = window.setInterval(() => {
      on = !on;
      document.title = on ? "⚠ 待审批 — BoenMind" : orig;
    }, 1200);
  }, []);
  useEffect(() => {
    const onVisible = () => {
      if (!document.hidden) stopTitleFlash();
    };
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener("focus", stopTitleFlash);
    return () => {
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener("focus", stopTitleFlash);
      stopTitleFlash();
    };
  }, [stopTitleFlash]);
  const notifyNewApproval = useCallback(
    (req: { approval_id: string; capability?: string }) => {
      if (!document.hidden) return;
      startTitleFlash();
      if ("Notification" in window) {
        if (Notification.permission === "granted") {
          const n = new Notification("BoenMind 等待审批", {
            body: `${req.capability ?? "工具调用"} 请求你的裁决`,
            tag: req.approval_id,
          });
          n.onclick = () => {
            window.focus();
            n.close();
          };
        } else if (Notification.permission === "default") {
          void Notification.requestPermission();
        }
      }
    },
    [startTitleFlash],
  );

  // 批准可达性轮询(2026-09-07 审批卡死根治):审批标记仅随回合 /v1 流下发,
  // 流到期或后台续跑回合无主流时,审批单永远无人可批,任务卡死在审批轮询。
  // 此通道每 2.5s 拉一次待裁决队列进抽屉(与流内标记
  // 按 approval_id 去重,双通道互为兜底)。
  useEffect(() => {
    const tick = async () => {
      try {
        const data = await api.approvals.list();
        for (const a of data.approvals ?? []) {
          if (handledApprovalsRef.current.has(a.approval_id)) continue;
          handledApprovalsRef.current.add(a.approval_id);
          // ADR-0030:裁决权在服务端——yolo 会话的审批由服务器在裁决点
          // 自动放行(审计 source=mode_auto),前端只呈现需要人工裁决的
          // 卡片(ask/plan);前端不再是裁决者
          notifyNewApproval(a);
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
      } catch {
        // 服务未起/重启窗口:静默,下一 tick 重试
      }
    };
    const iv = setInterval(tick, 2500);
    return () => clearInterval(iv);
    // notifyNewApproval 为稳定 useCallback(#25):加入依赖仅为本规检查
  }, [notifyNewApproval]);

  // 审批裁决 POST 公共实现(P1-2/P1-26 收口):失败回滚入队并从去重集摘除
  // (下一轮询兜底重试),不再静默吞错。走统一 client——非 2xx 由 req 抛错,
  // 401 由 client 统一跳登录(与主流同口径)。
  const postApprovalRespond = async (
    approvalId: string,
    decision: "approve" | "deny",
  ): Promise<boolean> => {
    try {
      await api.approvals.respond(approvalId, decision);
      return true;
    } catch (e) {
      console.warn(`审批裁决失败: ${approvalId} ${decision}`, e);
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
    // ADR-0055:本回合工具事件清空(每回合独立)
    setToolEvents([]);
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
    // ADR-0055:工具/审批元数据经 bm_event 结构化帧直送(不再解析正文标记)
    const toolHandlers = {
      onApproval: (req: ApprovalRequest) => approvalHandlerRef.current(req),
      onToolStarted: (t: ToolEvent) =>
        setToolEvents((cur) => [...cur, t]),
      onToolInvoked: (operationId: string, effect: string) =>
        setToolEvents((cur) =>
          cur.map((t) =>
            t.operationId === operationId
              ? { ...t, status: "done" as const }
              : t,
          ),
        ),
    };
    approvalHandlerRef.current = (req) => {
      // P1-26:流内到达即登记去重集,轮询通道不再重复入队
      handledApprovalsRef.current.add(req.approval_id);
      // ADR-0030:裁决权在服务端——yolo 会话的审批由服务器自动放行,
      // 流内到达的标记只做人工裁决呈现(ask/plan),前端不再代批
      notifyNewApproval(req);
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

    const requestEpoch = sessionEpochRef.current;
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
      // 门户会话失效:正向跳登录,不再让用户对着红条猜
      if (res.status === 401) {
        redirectToLogin();
      }
      if (res.status === 400) {
        const detail = await res.text().catch(() => "");
        // 结构化分支(issue #40):error.code 优先(webui.* 扩展码,见
        // boenmind-contracts/registry/extensions/webui.json);文案串匹配
        // 仅作旧服务端响应兜底,形状 = {"error":{message,type,code?}}
        let code = "";
        try {
          const parsed: { error?: { code?: string } } = JSON.parse(detail);
          code = parsed.error?.code ?? "";
        } catch {
          // 非 JSON 错误体:保持文本兜底
        }
        // W8:所选工作区未登记/已删除 → 清本地选择并给出可读提示
        // (不与「未知会话」重试混淆:换会话救不了坏工作区)
        if (
          code === "webui.workspace_unavailable" ||
          (!code && detail.includes("工作区"))
        ) {
          storage.remove(STORAGE_KEYS.ACTIVE_WORKSPACE);
          emit(BM_EVENTS.workspacesChanged);
          throw new Error(
            "所选工作目录不可用(可能已被删除):请重新选择,或到 设置→常规 检查",
          );
        }
        // P1-6: 仅当服务端明确返回「未知会话」时才清空记忆重试,其余 400 原样上屏
        if (
          code === "webui.session_unknown" ||
          (!code && (detail.includes("未知会话") || detail.includes("session")))
        ) {
          storage.remove(STORAGE_KEYS.SESSION);
          res = await doFetch(false);
        } else {
          throw new Error(`HTTP 400 ${detail.slice(0, 160)}`);
        }
      }
      if (!res.ok || !res.body) {
        const detail = await res.text().catch(() => "");
        throw new Error(`HTTP ${res.status} ${detail.slice(0, 160)}`);
      }
      // P1-7: 代数守卫——如果网络在途期间用户切走了会话或清空了会话,丢弃迟到的头部写回
      const newSid = res.headers.get("x-bm-session");
      if (newSid && sessionEpochRef.current === requestEpoch) {
        storage.set(STORAGE_KEYS.SESSION, newSid);
        // 会话目录已收归服务端(2026-09-08 三端一致批):标题由服务端在首条
        // user_message 落日志时回填(先于本响应头返回),前端只发刷新信号
        // 拉权威列表,不再本地记 bm_sessions 账
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
            bm_event?: BmEvent;
          };
          // ADR-0055:结构化元数据帧(工具/审批)——与 OpenAI 兼容帧共流,
          // 由独立 bm_event 键承载,不进正文。
          if (v.bm_event) {
            handleBmEvent(v.bm_event, toolHandlers);
            continue;
          }
          const d = v.choices?.[0]?.delta?.content;
          if (typeof d === "string" && d) pushText(d);
          // P1-11 配套:服务器失败/超时/中断不再谎报
          // finish stop+[DONE],改发 OpenAI 兼容错误帧——上屏告知用户
          else if (v.error?.message) {
            pushText(`\n[流式错误: ${v.error.message}]`);
          }
        }
      }
    } catch (e) {
      const aborted = e instanceof DOMException && e.name === "AbortError";
      pushText(
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
      setMsgWindow(MSG_WINDOW_DEFAULT);
      const epoch = ++sessionEpochRef.current;
      const sid = storage.get(STORAGE_KEYS.SESSION);
      if (!sid) return;
      try {
        const res = await api.sessionMessages(sid, { limit: HISTORY_PAGE });
        if (sessionEpochRef.current !== epoch) return; // 已切走,丢弃迟到响应
        setMessages(toThreadMessages(res.messages ?? []));
        historyCountRef.current = (res.messages ?? []).length;
        setHistoryMore({
          hasMore: res.has_more ?? false,
          loading: false,
          capped: false,
        });
      } catch (e) {
        // 回放失败(日志缺失/网络抖动):2026-09-08 审计修复——不再静默空白,
        // 上屏失败提示(与流内 [连接失败: …] 同款口径),不打断用户输入
        console.error("历史消息加载失败", e);
        if (sessionEpochRef.current === epoch) {
          setMessages([
            {
              role: "assistant",
              content: [
                {
                  type: "text",
                  text: `[历史消息加载失败: ${e instanceof Error ? e.message : String(e)};可重新切换会话或刷新重试]`,
                },
              ],
            } as ThreadMessageLike,
          ]);
        }
      }
    };
    window.addEventListener(BM_EVENTS.sessionSwitched, onSessionSwitched);
    return () =>
      window.removeEventListener(BM_EVENTS.sessionSwitched, onSessionSwitched);
  }, []);

  // 「打开/刷新 = 全新临时会话」(2026-09-08 三点会话语义,用户裁决):
  // 页面挂载不再恢复上次会话视图(storage 模块装载时已清会话指针),
  // 首条消息发出时才真正建服务端会话;历史回放统一走左侧列表切会话。
  // W4b:审批裁决(前端卡片按钮)→ /admin/approvals/{id}/respond
  // (与 /rpc 同一执行体;走 /admin 门户口径——已登录浏览器凭门户 Cookie,
  // 本机未设墙开放;后端鉴权见 auth::require_api_auth,issue #10)
  // P1-2:检查 res.ok;失败把审批单放回抽屉
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

  // 「加载更早消息」:skip=已加载条数,取前一页并前插
  // P1-28:在途期间切会话则丢弃响应(视图代守卫),不再前插到新会话上
  const loadOlder = async () => {
    const sid = storage.get(STORAGE_KEYS.SESSION);
    if (!sid || historyMore.loading) return;
    // #27:累计加载上限——到顶不再取,前端如实标 capped
    if (historyCountRef.current >= HISTORY_TOTAL_CAP) {
      setHistoryMore({ hasMore: false, loading: false, capped: true });
      return;
    }
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
      // #27:新取回的页即时入窗(否则用户要点两次才能看到)
      setMsgWindow((w) => w + older.length);
      setHistoryMore({
        hasMore: res.has_more ?? false,
        loading: false,
        // #27:取回后已达累计上限 = 本会话历史已取尽
        capped: res.has_more !== true && historyCountRef.current >= HISTORY_TOTAL_CAP,
      });
    } catch {
      setHistoryMore((h) => ({ ...h, loading: false }));
    }
  };

  return (
    <BoenmindRuntimeContext.Provider
      value={{
        pendingApprovals,
        respondApproval,
        toolEvents,
        editAndBranchMessage,
        regenerateMessage,
        history: {
          hasMore: historyMore.hasMore,
          loading: historyMore.loading,
          loadOlder: () => void loadOlder(),
          windowSize: msgWindow,
          expandOlder: () => setMsgWindow((w) => w + MSG_WINDOW_STEP),
          capped: historyMore.capped,
        },
      }}
    >
      <AssistantRuntimeProvider runtime={runtime}>
        {children}
      </AssistantRuntimeProvider>
    </BoenmindRuntimeContext.Provider>
  );
}
