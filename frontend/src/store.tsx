import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  type Dispatch,
  type ReactNode,
} from "react";
import { dateGroup } from "./lib/format";
import { historyToMessages, openMuxStream, respondApproval, rpc, type WireEvent } from "./lib/api";
import { fetchBackendModels } from "./lib/provider-api";
import { SEED_FILES, SEED_PLUGINS, SEED_SKILLS } from "./lib/seed";
import { applyDomSettings, loadSettings, saveSettings } from "./lib/storage";
import { toast } from "./lib/toast";
import { uid } from "./lib/format";
import type {
  AppState,
  Attachment,
  CatalogItem,
  ChatMessage,
  FileNode,
  PendingApproval,
  ReasoningLevel,
  Session,
  Settings,
  ToolCall,
  ViewId,
} from "./types";
import { DEFAULT_SETTINGS } from "./types";

type Action =
  | { type: "hydrate"; settings: Settings }
  | { type: "set-view"; view: ViewId }
  | { type: "toggle-settings" }
  | { type: "set-narrow"; narrow: boolean }
  | { type: "toggle-session-collapsed" }
  | { type: "set-session-collapsed"; value: boolean }
  | { type: "toggle-file-dock" }
  | { type: "set-file-dock"; value: boolean }
  | { type: "toggle-editor" }
  | { type: "select-session"; id: string }
  | { type: "new-session"; sessionId?: string }
  | { type: "rename-session"; id: string; title: string }
  | { type: "archive-session"; id: string; archived: boolean }
  | { type: "delete-session"; id: string }
  | { type: "set-session-tags"; id: string; tags: string[] }
  | { type: "toggle-tag-filter"; tag: string }
  | { type: "set-search"; q: string }
  | { type: "set-composer"; value: string }
  | { type: "add-attachment"; file: Attachment }
  | { type: "remove-attachment"; id: string }
  | { type: "set-model"; model: string }
  | { type: "set-reasoning"; reasoning: ReasoningLevel }
  | { type: "send"; sessionId?: string }
  | { type: "send-failed"; sessionId: string; text: string; attachments: Attachment[] }
  | { type: "stop" }
  | { type: "retry"; userId: string }
  | { type: "edit-user"; id: string; content: string }
  | { type: "patch-settings"; patch: Partial<Settings> }
  | { type: "set-cwd"; cwd: string[] }
  | { type: "open-file"; path: string }
  | { type: "set-draft"; value: string }
  | { type: "save-file" }
  | { type: "discard-draft" }
  | { type: "rename-file"; path: string; name: string }
  | { type: "delete-file"; path: string }
  | { type: "mkdir"; name: string }
  | { type: "upload"; name: string; content: string }
  | { type: "set-context-menu"; menu: AppState["contextMenu"] }
  | { type: "ask-confirm"; confirm: AppState["confirm"] }
  | { type: "close-confirm" }
  | { type: "open-modal"; modal: AppState["modal"] }
  | { type: "close-modal" }
  | { type: "uninstall"; kind: "skill" | "plugin"; id: string }
  | { type: "patch-catalog"; kind: "skill" | "plugin"; id: string; config: CatalogItem["config"] }
  | { type: "override-thinking"; id: string; open: boolean }
  | { type: "set-connected"; value: boolean }
  | { type: "backend-boot" }
  | { type: "backend-model-groups"; groups: Array<{ provider: string; models: string[] }> }
  | { type: "backend-list"; sessions: Session[] }
  | { type: "backend-clear-sessions" }
  | { type: "backend-set-session-running"; id: string; running: boolean }
  | { type: "backend-add-session"; id: string; title: string; preview: string; running?: boolean }
  | { type: "backend-set-session-title"; id: string; title: string; preview?: string }
  | { type: "backend-set-session-preview"; id: string; preview: string; updatedAt?: number }
  | { type: "wipe-local-session"; id: string }
  | { type: "delete-session-local"; id: string }
  | { type: "apply-history"; sessionId: string; messages: ChatMessage[] }
  | { type: "backend-event"; sessionId: string; event: WireEvent }
  | { type: "approval-requested"; approval: PendingApproval }
  | { type: "approval-resolved"; rpcId: string }
  | { type: "approval-respond"; rpcId: string; outcome: "allowed-once" | "rejected" };

function walkFiles(nodes: FileNode[], fn: (n: FileNode, parent: FileNode[] | null) => void, parent: FileNode[] | null = null) {
  for (const n of nodes) {
    fn(n, parent);
    if (n.children) walkFiles(n.children, fn, n.children);
  }
}

function findFile(nodes: FileNode[], path: string): FileNode | null {
  let found: FileNode | null = null;
  walkFiles(nodes, (n) => {
    if (n.path === path) found = n;
  });
  return found;
}

function dirAt(nodes: FileNode[], cwd: string[]): FileNode[] {
  let cur = nodes;
  for (const seg of cwd) {
    const next = cur.find((n) => n.name === seg && n.kind === "dir");
    if (!next?.children) return cur;
    cur = next.children;
  }
  return cur;
}

function initialState(settings: Settings): AppState {
  return {
    view: "chat",
    returnView: "chat",
    sessions: [],
    activeSessionId: null,
    messages: {},
    sessionCollapsed: false,
    fileDockCollapsed: false,
    editorCollapsed: false,
    narrow: false,
    connected: true,
    streaming: false,
    streamStartedAt: null,
    settings,
    selectedTags: [],
    sessionSearch: "",
    composer: "",
    composerAttachments: [],
    model: settings.defaultModel,
    reasoning: settings.defaultReasoning,
    usage: { used: 12, limit: 40 },
    pendingApprovals: [],
    files: structuredClone(SEED_FILES),
    cwd: [],
    openFilePath: null,
    openFileDraft: "",
    openFileDirty: false,
    skills: structuredClone(SEED_SKILLS),
    plugins: structuredClone(SEED_PLUGINS),
    contextMenu: null,
    confirm: null,
    modal: null,
    thinkingOverrides: {},
    booted: false,
    backendModels: [],
  };
}

function reducer(state: AppState, action: Action): AppState {
  switch (action.type) {
    case "hydrate":
      return { ...state, settings: action.settings, model: action.settings.defaultModel, reasoning: action.settings.defaultReasoning };
    case "set-view":
      return {
        ...state,
        view: action.view,
        returnView: action.view === "settings" ? state.returnView : action.view,
        fileDockCollapsed: action.view === "chat" ? state.fileDockCollapsed : true,
      };
    case "toggle-settings":
      if (state.view === "settings") {
        return { ...state, view: state.returnView };
      }
      return {
        ...state,
        returnView: state.view,
        view: "settings",
        fileDockCollapsed: true,
      };
    case "set-narrow":
      return { ...state, narrow: action.narrow };
    case "toggle-session-collapsed":
      return { ...state, sessionCollapsed: !state.sessionCollapsed };
    case "set-session-collapsed":
      return { ...state, sessionCollapsed: action.value };
    case "toggle-file-dock":
      return { ...state, fileDockCollapsed: !state.fileDockCollapsed };
    case "set-file-dock":
      return { ...state, fileDockCollapsed: action.value };
    case "toggle-editor":
      return { ...state, editorCollapsed: !state.editorCollapsed };
    case "select-session": {
      // 流式状态跟随目标会话的 running 标记（host 帧维护）：切到非流式会话立即
      // 复位（回归：曾不重置，B 会被误标 streaming 直到下一个 host 帧）。
      const running = state.sessions.some((s) => s.id === action.id && s.running);
      return {
        ...state,
        activeSessionId: action.id,
        view: "chat",
        thinkingOverrides: {},
        streaming: running,
        streamStartedAt: running ? state.streamStartedAt : null,
      };
    }
    case "new-session": {
      // Create RPC 在 cmd 命令层；WS `subscribed` / host/session-added 会补列表。
      const sid = action.sessionId ?? uid("s");
      return {
        ...state,
        sessions: [...state.sessions, { id: sid, title: "新会话", preview: "还没有消息", updatedAt: Date.now(), tags: [], archived: false }],
        activeSessionId: sid,
        messages: { ...state.messages, [sid]: [] },
        view: "chat",
      };
    }
    case "rename-session":
      return {
        ...state,
        sessions: state.sessions.map((s) => (s.id === action.id ? { ...s, title: action.title } : s)),
      };
    case "archive-session":
      return {
        ...state,
        sessions: state.sessions.map((s) => (s.id === action.id ? { ...s, archived: action.archived } : s)),
        activeSessionId:
          action.archived && state.activeSessionId === action.id
            ? state.sessions.find((s) => !s.archived && s.id !== action.id)?.id ?? null
            : state.activeSessionId,
      };
    case "delete-session":
    case "delete-session-local": {
      // 两者 reducer 语义相同（本地移除）；delete-session 的 RPC 在 cmd 命令层，
      // host/session-removed 到达时用本地版（后端已删，不再回发 RPC）。
      const sessions = state.sessions.filter((s) => s.id !== action.id);
      const messages = { ...state.messages };
      delete messages[action.id];
      return {
        ...state,
        sessions,
        messages,
        activeSessionId: state.activeSessionId === action.id ? sessions.find((s) => !s.archived)?.id ?? null : state.activeSessionId,
      };
    }
    case "set-session-tags":
      return {
        ...state,
        sessions: state.sessions.map((s) => (s.id === action.id ? { ...s, tags: action.tags } : s)),
      };
    case "toggle-tag-filter": {
      const on = state.selectedTags.includes(action.tag);
      return { ...state, selectedTags: on ? state.selectedTags.filter((t) => t !== action.tag) : [...state.selectedTags, action.tag] };
    }
    case "set-search":
      return { ...state, sessionSearch: action.q };
    case "set-composer":
      return { ...state, composer: action.value };
    case "add-attachment":
      return { ...state, composerAttachments: [...state.composerAttachments, action.file] };
    case "remove-attachment":
      return { ...state, composerAttachments: state.composerAttachments.filter((a) => a.id !== action.id) };
    case "set-model":
      // RPC 同步在 cmd 命令层（keep reducer 纯）。
      return { ...state, model: action.model };
    case "set-reasoning":
      return { ...state, reasoning: action.reasoning };
    case "send": {
      if (!state.composer.trim() && state.composerAttachments.length === 0) return state;
      // 会话 id 由命令层决定（action.sessionId），reducer 不自行生成，避免
      // StrictMode 双跑出现不同 uid / 与命令层不一致。
      const sessionId = action.sessionId ?? state.activeSessionId;
      if (!sessionId) return state;
      const text = state.composer;
      const attachments = state.composerAttachments;
      const sessions = state.sessions;
      const messages = state.messages;
      const user: ChatMessage = {
        id: uid("m"),
        role: "user",
        content: text,
        createdAt: Date.now(),
        attachments,
      };
      const asst: ChatMessage = {
        id: uid("m"),
        role: "assistant",
        content: "…",
        createdAt: Date.now(),
        model: state.model,
        thinking: state.reasoning === "off" ? undefined : "",
      };
      const list = [...(messages[sessionId] ?? []), user, asst];
      return {
        ...state,
        sessions: sessions.map((s) =>
          s.id === sessionId ? { ...s, preview: text || "附件", updatedAt: Date.now(), title: s.title === "新会话" ? text.slice(0, 18) || s.title : s.title } : s,
        ),
        activeSessionId: sessionId,
        messages: { ...messages, [sessionId]: list },
        composer: "",
        composerAttachments: [],
        streaming: false,
        streamStartedAt: null,
        thinkingOverrides: {},
      };
    }
    case "send-failed": {
      // 发送失败回滚：撤掉 send 乐观插入的 user + assistant 占位对，文本/附件
      // 退回输入框（回归：曾只 toast 不回滚，占位符永久残留）。
      const list = [...(state.messages[action.sessionId] ?? [])];
      if (list.length && list[list.length - 1].role === "assistant" && list[list.length - 1].content === "…") {
        list.pop();
      }
      if (list.length && list[list.length - 1].role === "user") {
        list.pop();
      }
      return {
        ...state,
        composer: action.text,
        composerAttachments: action.attachments,
        messages: { ...state.messages, [action.sessionId]: list },
      };
    }
    case "stop": {
      const sid = state.activeSessionId;
      if (!sid) return state;
      return {
        ...state,
        streaming: false,
        streamStartedAt: null,
        messages: {
          ...state.messages,
          [sid]: (state.messages[sid] ?? []).map((m, i, arr) =>
            i === arr.length - 1 && m.role === "assistant" && m.content === "…"
              ? { ...m, content: "已停止生成。" }
              : m,
          ),
        },
      };
    }
    case "retry": {
      const sid = state.activeSessionId;
      if (!sid) return state;
      const list = state.messages[sid] ?? [];
      const idx = list.findIndex((m) => m.id === action.userId);
      if (idx < 0) return state;
      const asst: ChatMessage = {
        id: uid("m"),
        role: "assistant",
        content: "…",
        createdAt: Date.now(),
        model: state.model,
        thinking: state.reasoning === "off" ? undefined : "",
      };
      return {
        ...state,
        messages: { ...state.messages, [sid]: [...list.slice(0, idx + 1), asst] },
        streaming: false,
        streamStartedAt: null,
      };
    }
    case "edit-user":
      return { ...state, composer: action.content };
    case "patch-settings": {
      const settings = { ...state.settings, ...action.patch };
      return {
        ...state,
        settings,
        model: action.patch.defaultModel ?? state.model,
        reasoning: action.patch.defaultReasoning ?? state.reasoning,
      };
    }
    case "set-cwd":
      return { ...state, cwd: action.cwd };
    case "open-file": {
      const f = findFile(state.files, action.path);
      if (!f || f.kind === "dir") return state;
      return {
        ...state,
        openFilePath: action.path,
        openFileDraft: f.content ?? "",
        openFileDirty: false,
        editorCollapsed: false,
      };
    }
    case "set-draft":
      return { ...state, openFileDraft: action.value, openFileDirty: true };
    case "save-file": {
      if (!state.openFilePath) return state;
      const files = structuredClone(state.files);
      const f = findFile(files, state.openFilePath);
      if (f) f.content = state.openFileDraft;
      return { ...state, files, openFileDirty: false };
    }
    case "discard-draft": {
      if (!state.openFilePath) return state;
      const f = findFile(state.files, state.openFilePath);
      return { ...state, openFileDraft: f?.content ?? "", openFileDirty: false, openFilePath: state.openFilePath };
    }
    case "rename-file": {
      const files = structuredClone(state.files);
      const f = findFile(files, action.path);
      if (f) {
        const parent = action.path.includes("/") ? action.path.slice(0, action.path.lastIndexOf("/")) : "";
        f.name = action.name;
        f.path = parent ? `${parent}/${action.name}` : action.name;
      }
      return { ...state, files, openFilePath: state.openFilePath === action.path ? f?.path ?? null : state.openFilePath };
    }
    case "delete-file": {
      const files = structuredClone(state.files);
      const remove = (arr: FileNode[]): FileNode[] =>
        arr.filter((n) => n.path !== action.path).map((n) => (n.children ? { ...n, children: remove(n.children) } : n));
      return {
        ...state,
        files: remove(files),
        openFilePath: state.openFilePath === action.path ? null : state.openFilePath,
        openFileDirty: state.openFilePath === action.path ? false : state.openFileDirty,
      };
    }
    case "mkdir": {
      const files = structuredClone(state.files);
      const target = dirAt(files, state.cwd);
      const path = [...state.cwd, action.name].join("/");
      target.push({ id: uid("d"), name: action.name, path, kind: "dir", children: [] });
      return { ...state, files };
    }
    case "upload": {
      const files = structuredClone(state.files);
      const target = dirAt(files, state.cwd);
      const path = [...state.cwd, action.name].join("/") || action.name;
      target.push({ id: uid("f"), name: action.name, path, kind: "text", content: action.content });
      return { ...state, files };
    }
    case "set-context-menu":
      return { ...state, contextMenu: action.menu };
    case "ask-confirm":
      return { ...state, confirm: action.confirm };
    case "close-confirm":
      return { ...state, confirm: null };
    case "open-modal":
      return { ...state, modal: action.modal };
    case "close-modal":
      return { ...state, modal: null };
    case "uninstall": {
      if (action.kind === "skill") return { ...state, skills: state.skills.filter((s) => s.id !== action.id) };
      return { ...state, plugins: state.plugins.filter((s) => s.id !== action.id) };
    }
    case "patch-catalog": {
      const patch = (list: CatalogItem[]) => list.map((i) => (i.id === action.id ? { ...i, config: action.config } : i));
      return action.kind === "skill" ? { ...state, skills: patch(state.skills) } : { ...state, plugins: patch(state.plugins) };
    }
    case "override-thinking":
      return { ...state, thinkingOverrides: { ...state.thinkingOverrides, [action.id]: action.open } };
    case "set-connected":
      return { ...state, connected: action.value };
    case "backend-boot":
      return { ...state, booted: true };
    case "backend-model-groups":
      return { ...state, backendModels: action.groups };
    case "backend-list":
      return { ...state, sessions: action.sessions };
    case "backend-clear-sessions":
      return { ...state, sessions: [], activeSessionId: null, messages: {} };
    case "backend-set-session-running":
      return {
        ...state,
        sessions: state.sessions.map((s) => (s.id === action.id ? { ...s, running: action.running } : s)),
        streaming: action.running && state.activeSessionId === action.id,
        streamStartedAt: action.running && state.activeSessionId === action.id ? Date.now() : state.streamStartedAt,
      };
    case "backend-add-session": {
      // host/session-added：会话不在本地列表则新增（避免只改标题丢新增）。
      const exists = state.sessions.some((s) => s.id === action.id);
      if (exists) return state;
      return {
        ...state,
        sessions: [
          { id: action.id, title: action.title, preview: action.preview, updatedAt: Date.now(), tags: [], archived: false, running: action.running },
          ...state.sessions,
        ],
      };
    }
    case "backend-set-session-title":
      return {
        ...state,
        sessions: state.sessions.map((s) =>
          s.id === action.id ? { ...s, title: action.title, preview: action.preview ?? s.preview } : s,
        ),
      };
    case "backend-set-session-preview":
      return {
        ...state,
        sessions: state.sessions.map((s) =>
          s.id === action.id ? { ...s, preview: action.preview, updatedAt: action.updatedAt ?? s.updatedAt } : s,
        ),
      };
    case "wipe-local-session": {
      const messages = { ...state.messages };
      delete messages[action.id];
      return { ...state, messages };
    }
    case "apply-history":
      return { ...state, messages: { ...state.messages, [action.sessionId]: action.messages } };
    case "backend-event": {
      const sid = action.sessionId;
      const ev = action.event;
      const cur = state.messages[sid] ?? [];
      const d = ev.data;
      switch (ev.type) {
        case "user/message": {
          const blocks = (d.content as Array<{ type?: string; text?: string }>) ?? [];
          const text = blocks
            .filter((b) => b?.type === "text")
            .map((b) => b.text ?? "")
            .join("");
          const exists = cur.some((m) => m.id === (d.id as string));
          if (exists) return state;
          // 乐观插入的 user 消息（同文 & 10s 内）→ 视为本人回显，跳过防重。
          const last = cur[cur.length - 1];
          if (last && last.role === "user" && last.content === text && ev.time - last.createdAt < 10000) return state;
          return {
            ...state,
            messages: {
              ...state.messages,
              [sid]: [...cur, { id: (d.id as string) ?? uid("m"), role: "user", content: text, createdAt: ev.time }],
            },
          };
        }
        case "assistant/chunk": {
          const chunk = d.chunk as { type?: string; text?: string } | undefined;
          const ctype = chunk?.type;
          const textChunk = chunk?.text ?? "";
          const last = cur[cur.length - 1];
          if (!last || last.role !== "assistant") return state;
          if (ctype === "text-delta") {
            return {
              ...state,
              messages: {
                ...state.messages,
                [sid]: cur.map((m: ChatMessage, i: number) =>
                  i === cur.length - 1 ? { ...m, content: m.content === "…" ? textChunk : m.content + textChunk } : m,
                ),
              },
            };
          }
          if (ctype === "reasoning-delta") {
            return {
              ...state,
              messages: {
                ...state.messages,
                [sid]: cur.map((m: ChatMessage, i: number) => (i === cur.length - 1 ? { ...m, thinking: (m.thinking ?? "") + textChunk } : m)),
              },
            };
          }
          if (ctype === "tool-call") {
            const tc = chunk as unknown as Record<string, unknown>;
            const entry: ToolCall = {
              id: (tc.callId as string) ?? uid("t"),
              name: (tc.name as string) ?? "tool",
              summary: (tc.arguments as string)?.slice(0, 48) ?? "",
              status: "running",
            };
            return {
              ...state,
              messages: {
                ...state.messages,
                [sid]: cur.map((m: ChatMessage, i: number) => {
                  if (i !== cur.length - 1) return m;
                  const tools = m.tools ? (m.tools.find((t) => t.id === entry.id) ? m.tools : [...m.tools, entry]) : [entry];
                  return { ...m, tools };
                }),
              },
            };
          }
          return state;
        }
        case "assistant/message": {
          const msg = d.message as { id?: string; content?: unknown[] } | undefined;
          if (!msg) return state;
          const content = (msg.content as Array<{ type?: string; text?: string }>) ?? [];
          const text = content.filter((b) => b?.type === "text").map((b) => b.text ?? "").join("");
          const think = content.filter((b) => b?.type === "reasoning").map((b) => b.text ?? "").join("");
          const last = cur[cur.length - 1];
          if (last && last.role === "assistant") {
            return {
              ...state,
              messages: {
                ...state.messages,
                [sid]: cur.map((m: ChatMessage, i: number) =>
                  i === cur.length - 1 ? { ...m, content: text || m.content, thinking: think || m.thinking } : m,
                ),
              },
            };
          }
          return state;
        }
        case "tool/call": {
          const entry: ToolCall = {
            id: (d.callId as string) ?? uid("t"),
            name: (d.name as string) ?? "tool",
            summary: (d.arguments as string)?.slice(0, 48) ?? "",
            status: "running",
          };
          const last = cur[cur.length - 1];
          if (!last || last.role !== "assistant") return state;
          return {
            ...state,
            messages: {
              ...state.messages,
              [sid]: cur.map((m: ChatMessage, i: number) => {
                if (i !== cur.length - 1) return m;
                const tools = m.tools ? (m.tools.find((t) => t.id === entry.id) ? m.tools : [...m.tools, entry]) : [entry];
                return { ...m, tools };
              }),
            },
          };
        }
        case "tool/result": {
          const source = (d.message as { source?: { callId?: string } })?.source;
          if (!source?.callId) return state;
          return {
            ...state,
            messages: {
              ...state.messages,
              [sid]: cur.map((m: ChatMessage, i: number) => {
                if (i !== cur.length - 1) return m;
                const tools = m.tools?.map((t) => (t.id === source.callId ? { ...t, status: "done" as const } : t));
                return tools ? { ...m, tools } : m;
              }),
            },
          };
        }
        case "turn/end": {
          const reason = (d.reason as { kind?: string })?.kind;
          const last = cur[cur.length - 1];
          const withEnd = last && last.role === "assistant"
            ? cur.map((m, i) => {
                if (i !== cur.length - 1) return m;
                // 回合结束：占位 "…" 且无实际内容 → 收尾文案（mock/空回复）。
                const content = m.content === "…" ? (reason === "aborted" ? "已停止生成。" : "(空回复)") : m.content;
                return { ...m, content };
              })
            : cur;
          return {
            ...state,
            sessions: state.sessions.map((s) =>
              s.id === sid ? { ...s, preview: (last?.content ?? s.preview).slice(0, 60), updatedAt: ev.time } : s,
            ),
            messages: { ...state.messages, [sid]: withEnd },
            // 回合结束一律复位流式（回归：曾仅 aborted 复位，live 状态完全依赖
            // host 帧时序，host 帧延迟即卡在「生成中」）。
            streaming: false,
            streamStartedAt: null,
          };
        }
        default:
          return state;
      }
    }
    case "approval-requested": {
      // mux 重连会重放仍 pending 的审批（rpcId 原样复用）→ 按 rpcId 幂等去重。
      if (state.pendingApprovals.some((a) => a.rpcId === action.approval.rpcId)) return state;
      return { ...state, pendingApprovals: [...state.pendingApprovals, action.approval] };
    }
    case "approval-resolved":
      return { ...state, pendingApprovals: state.pendingApprovals.filter((a) => a.rpcId !== action.rpcId) };
    case "approval-respond":
      // RPC 副作用在 cmd 命令层；reducer 无事可做。
      return state;
    default:
      return state;
  }
}

const Ctx = createContext<{
  state: AppState;
  dispatch: Dispatch<Action>;
  visibleSessions: Session[];
  allTags: string[];
  activeMessages: ChatMessage[];
} | null>(null);

export function StoreProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, DEFAULT_SETTINGS, initialState);
  // 命令层/异步回调读最新状态用（提交后写入；不再 render 期写模块级对象）。
  const latestRef = useRef(state);
  useEffect(() => {
    latestRef.current = state;
  });

  useEffect(() => {
    const s = loadSettings();
    dispatch({ type: "hydrate", settings: s });
    applyDomSettings(s);
  }, []);

  useEffect(() => {
    applyDomSettings(state.settings);
    saveSettings(state.settings);
  }, [state.settings]);

  useEffect(() => {
    const mq = window.matchMedia("(max-width: 639px)");
    const apply = () => {
      const narrow = mq.matches;
      dispatch({ type: "set-narrow", narrow });
      if (narrow) dispatch({ type: "set-file-dock", value: true });
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, []);

  // 后端引导：session.list 拉会话列表；随后订阅 mux 实时事件流。
  useEffect(() => {
    let mounted = true;
    const boot = async () => {
      const list = await rpc<{ items: Array<{ sessionId: string; updatedAt: string; running: boolean; blank: boolean; cwd: string }> }>("session.list", {});
      if (!mounted) return;
      if (list.ok) {
        const sessions: Session[] = (list.value.items ?? []).map((it) => ({
          id: it.sessionId,
          title: it.sessionId.slice(0, 8),
          preview: it.blank ? "还没有消息" : "…",
          updatedAt: it.updatedAt ? Date.parse(it.updatedAt) || Date.now() : Date.now(),
          tags: [],
          archived: false,
          running: it.running,
        }));
        dispatch({ type: "backend-list", sessions });
        dispatch({ type: "backend-boot" });
      } else {
        // 后端不可达 → 空态提示，保留 booted 以便渲染。
        dispatch({ type: "backend-clear-sessions" });
        dispatch({ type: "backend-boot" });
        dispatch({ type: "set-connected", value: false });
        toast.error("无法连接本地服务");
      }
    };
    void boot();
    return () => {
      mounted = false;
    };
  }, []);

  // 后端提供商模型目录（llm.models）→ 聊天下拉事实源（本地 providers 为空时用后端）。
  useEffect(() => {
    let mounted = true;
    void fetchBackendModels().then((groups) => {
      if (!mounted || !groups) return;
      const flat: { provider: string; models: string[] }[] = groups.map((g) => ({
        provider: g.name || g.id,
        models: g.models.map((m) => m.id),
      }));
      dispatch({ type: "backend-model-groups", groups: flat });
    });
    return () => {
      mounted = false;
    };
  }, []);

  // session 事件流（mux WS）：user/assistant 实时增量 + 审批请求。
  useEffect(() => {
    const stream = openMuxStream((frame) => {
      const method = frame.method;
      const p = frame.payload as { sessionId?: string; event?: WireEvent; lastSeq?: number } & Record<string, unknown>;
      switch (method) {
        case "session/event": {
          const ev = p.event;
          const sid = p.sessionId;
          if (!sid || !ev) return;
          dispatch({ type: "backend-event", sessionId: sid, event: ev });
          return;
        }
        case "session/subscribed": {
          const sid = p.sessionId;
          if (!sid) return;
          dispatch({ type: "set-connected", value: true });
          void loadHistory(sid, dispatch, () => latestRef.current);
          return;
        }
        case "approval/requested": {
          // 工具审批：不处理会话会永久挂起（agent 在等 /api/respond）。
          const approval = p as unknown as { sessionId?: string; approvalId?: string; toolName?: string; callId?: string; reason?: string };
          if (!approval.sessionId || !approval.approvalId) return;
          dispatch({
            type: "approval-requested",
            approval: {
              rpcId: frame.rpcId,
              sessionId: approval.sessionId,
              approvalId: approval.approvalId,
              toolName: approval.toolName ?? "unknown-tool",
              callId: approval.callId,
              reason: approval.reason,
            },
          });
          return;
        }
        case "approval/resolved": {
          // 后端确认（本端或另一客户端已应答）→ 移除对应审批项。
          dispatch({ type: "approval-resolved", rpcId: frame.rpcId });
          return;
        }
        case "question/requested":
          // 后端当前无生产登记点（仅测试钩子可注入）；出现即显式暴露，不静默丢弃。
          console.warn("question/requested frame received but question UI is not implemented yet", frame);
          return;
        default:
          return;
      }
    });
    return () => stream.close();
  }, []);

  // host 事件流（host WS）：会话增删 / running 状态。
  useEffect(() => {
    const stream = openMuxStream((frame) => {
      const method = frame.method;
      const p = frame.payload as {
        sessionId?: string;
        blank?: boolean;
        running?: boolean;
        archivedSessionIds?: string[];
      } & Record<string, unknown>;
      switch (method) {
        case "host/session-added": {
          const sid = p.sessionId;
          if (!sid) return;
          const blank = Boolean(p.blank);
          dispatch({ type: "backend-add-session", id: sid, title: sid.slice(0, 8), preview: blank ? "还没有消息" : "…", running: Boolean(p.running) });
          if (latestRef.current.activeSessionId === sid) void loadHistory(sid, dispatch, () => latestRef.current);
          return;
        }
        case "host/session-status": {
          const sid = p.sessionId;
          if (!sid) return;
          dispatch({ type: "backend-set-session-running", id: sid, running: Boolean(p.running) });
          return;
        }
        case "host/session-removed": {
          const sid = p.sessionId;
          if (!sid) return;
          dispatch({ type: "delete-session-local", id: sid });
          return;
        }
        default:
          return;
      }
    }, "/api/events.host");
    return () => stream.close();
  }, []);

  // 切换会话 → 拉历史（已加载过的不重拉）。
  const loadedRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (!state.activeSessionId || state.booted === false) return;
    const sid = state.activeSessionId;
    if (loadedRef.current.has(sid)) return;
    loadedRef.current.add(sid);
    void loadHistory(sid, dispatch, () => latestRef.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.activeSessionId, state.booted]);

  const allTags = useMemo(() => {
    const set = new Set<string>();
    state.sessions.forEach((s) => s.tags.forEach((t) => set.add(t)));
    return [...set];
  }, [state.sessions]);

  const visibleSessions = useMemo(() => {
    const q = state.sessionSearch.trim().toLowerCase();
    return state.sessions.filter((s) => {
      if (s.archived) return false;
      if (q && !`${s.title} ${s.preview}`.toLowerCase().includes(q)) return false;
      if (state.selectedTags.length && !state.selectedTags.some((t) => s.tags.includes(t))) return false;
      return true;
    });
  }, [state.sessions, state.sessionSearch, state.selectedTags]);

  const activeMessages = state.activeSessionId ? (state.messages[state.activeSessionId] ?? []) : [];

  // 命令层：reducer 保持纯，所有需要后端 RPC 的「用户动作」在此统一收口发送
  // （单次执行，不受 StrictMode 双跑 reducer 影响）。
  const cmd = useCallback((action: Action) => {
    switch (action.type) {
      case "new-session": {
        // create 幂等；后端 host/session-added 会补列表（含 blank 状态）。
        // sid 由命令层生成并注入 action 再透传 reducer（reducer 不自造 id，
        // 避免 StrictMode 双跑得到不同 uid）。
        const sid = action.sessionId ?? uid("s");
        void rpcToast("session.create", { sessionId: sid }, "创建会话失败");
        return dispatch(action.sessionId ? action : { ...action, sessionId: sid });
      }
      case "rename-session":
        void rpcToast("session.rename", { sessionId: action.id, title: action.title }, "重命名失败");
        break;
      case "archive-session":
        if (action.archived) void rpcToast("workspace.archiveSession", { sessionId: action.id }, "归档失败");
        break;
      case "delete-session":
        void rpcToast("session.delete", { sessionId: action.id }, "删除会话失败");
        break;
      case "set-model": {
        const st = latestRef.current;
        const sid = st.activeSessionId;
        // 反查模型所属 provider（本地配置优先，其次后端装配组）。
        let provider = "";
        for (const p of st.settings.providers) if (p.models.includes(action.model)) { provider = p.id; break; }
        if (!provider) {
          for (const g of st.backendModels) if (g.models.includes(action.model)) { provider = g.provider; break; }
        }
        if (sid) {
          void rpcToast("session.selectModel", {
            sessionId: sid,
            provider: provider || st.settings.defaultProvider || "mock",
            model: action.model,
          }, "模型切换未同步");
        }
        // 全局默认也同步（模型选择是产品意图，不只本次会话）。
        if (st.settings.defaultModel !== action.model) {
          dispatch({ type: "patch-settings", patch: { defaultModel: action.model, defaultProvider: provider || st.settings.defaultProvider } });
        }
        break;
      }
      case "retry": {
        const sid = latestRef.current.activeSessionId;
        const list = latestRef.current.messages[sid ?? ""] ?? [];
        const idx = list.findIndex((m) => m.id === action.userId);
        if (sid && idx >= 0) {
          void rpc("session.prompt", { sessionId: sid, content: [{ type: "text", text: list[idx].content }] }).catch(() =>
            toast.error("重试失败：无法连接本地服务"),
          );
        }
        break;
      }
      case "stop": {
        const sid = latestRef.current.activeSessionId;
        if (sid && latestRef.current.streaming) {
          void rpcToast("session.cancel", { sessionId: sid }, "停止失败");
        }
        break;
      }
      case "approval-respond": {
        // 审批应答：accepted 才移除（失败保留卡片供重试，不静默丢）。
        const approval = latestRef.current.pendingApprovals.find((a) => a.rpcId === action.rpcId);
        if (!approval) {
          dispatch({ type: "approval-resolved", rpcId: action.rpcId });
          break;
        }
        void respondApproval(approval, action.outcome).then((accepted) => {
          if (accepted) {
            dispatch({ type: "approval-resolved", rpcId: approval.rpcId });
            toast.success(action.outcome === "allowed-once" ? "已允许本次执行" : "已拒绝");
          } else {
            toast.error("审批应答未送达，请重试");
          }
        });
        break;
      }
      default:
        break;
    }
    dispatch(action);
  }, [dispatch]);

  const value = useMemo(
    () => ({ state, dispatch: cmd, visibleSessions, allTags, activeMessages }),
    [state, cmd, visibleSessions, allTags, activeMessages],
  );

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

/** 命令层统一 RPC 出口：失败 toast（不再静默吞错；网络异常与信封错误都覆盖）。 */
async function rpcToast(endpoint: string, payload: unknown, failMsg: string) {
  try {
    const r = await rpc(endpoint, payload);
    if (!r.ok) toast.error(`${failMsg}：${r.error.message}`);
  } catch {
    toast.error(`${failMsg}：无法连接本地服务`);
  }
}

/** 每会话的 history 拉取代际：只允许最新一次生效（过期响应丢弃，
 * 防止发送后立刻切走时旧历史回包覆盖乐观消息）。 */
const historyGen = new Map<string, number>();

async function loadHistory(sessionId: string, dispatch: Dispatch<Action>, getState: () => AppState) {
  const gen = (historyGen.get(sessionId) ?? 0) + 1;
  historyGen.set(sessionId, gen);
  const hist = await rpc<{ events: Array<{ event: WireEvent }> }>("session.history", { sessionId });
  if (historyGen.get(sessionId) !== gen) return; // 已被更新的拉取取代
  if (!hist.ok) return;
  const events = (hist.value.events ?? []).map((e) => e.event);
  const msgs = historyToMessages(events);
  // 若该会话正在流式（乐观 user/assistant 已插入），跳过覆盖——实时 chunk 会续写；
  // 否则 wipe + 回放历史。状态在 await 后现读（读取器穿透最新提交）。
  const st = getState();
  if (st.streaming && st.activeSessionId === sessionId) return;
  dispatch({ type: "wipe-local-session", id: sessionId });
  if (msgs.length) {
    dispatch({ type: "apply-history", sessionId, messages: msgs });
  }
}

export function useStore() {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("store");
  return ctx;
}

/** 聊天发送/停止动作（真实 RPC；失败回滚乐观插入）。 */
export function useChatActions() {
  const { state, dispatch } = useStore();

  return useCallback(() => {
    if (state.streaming) {
      // Stop：cancel 由命令层 stop 统一发送（回归：曾在此直发一次 + cmd 层再发一次）。
      dispatch({ type: "stop" });
      toast.info("已停止生成");
      return;
    }
    if (!state.composer.trim() && state.composerAttachments.length === 0) return;
    const sessionId = state.activeSessionId ?? uid("s");
    const text = state.composer;
    const attachments = state.composerAttachments;
    const fail = () => dispatch({ type: "send-failed", sessionId, text, attachments });
    void (async () => {
      const c = await rpc<{ sessionId?: string }>("session.create", { sessionId });
      if (!c.ok) {
        toast.error(`发送失败：${c.error.message}`);
        fail();
        return;
      }
      const id = c.value?.sessionId ?? sessionId;
      const p = await rpc("session.prompt", { sessionId: id, content: [{ type: "text", text }] });
      if (!p.ok) {
        if (p.error.code === "agent-busy") toast.info("会话正在生成中");
        else toast.error(`发送失败：${p.error.message}`);
        fail();
      }
    })().catch(() => {
      toast.error("发送失败：无法连接本地服务");
      fail();
    });
    // 命令层持有最终 sessionId（新会话由这里生成，reducer 只用它）。
    dispatch({ type: "send", sessionId });
  }, [state.streaming, state.composer, state.composerAttachments, state.activeSessionId, dispatch]);
}

export { dateGroup };
export type { Action };
