/**
 * 全局状态：导航、会话、聊天流、文件区、后端健康状态。
 */
import { create } from "zustand";
import { api, type AppConfig, type ChatStreamEvent, type FileEntry, type HealthInfo, type Message, type Session, type ToolCall } from "@/api/client";
import i18n, { applyLang, isLang } from "@/i18n";
import { toast } from "sonner";

export type NavKey = "chat" | "gallery" | "knowledge" | "settings";
export type SettingsTab = "appearance" | "providers" | "workspace" | "plugins" | "skills" | "about";

/** 流式中的工具调用（isError 未定前为执行中状态） */
export interface StreamingToolCall {
  id: string;
  name: string;
  args: unknown;
  isError: boolean;
  /** 已收到 ToolCallEnd（执行结束，颜色取 isError；未结束时保持中性灰） */
  done: boolean;
}

/**
 * 配置默认模型（"providerId::modelId"）：全局 default_model → 所属提供商，
 * 无则取第一个提供商的首个模型；未配置任何提供商时为 null。
 */
export function defaultModelValue(config: AppConfig): string | null {
  const def = config.default_model;
  if (def) {
    const p = config.providers.find((p) => p.default_model === def || p.models.includes(def));
    if (p) return `${p.id}::${def}`;
  }
  const p = config.providers[0];
  if (p?.models[0]) return `${p.id}::${p.models[0]}`;
  return null;
}

interface AppStore {
  // 导航
  activeNav: NavKey;
  settingsTab: SettingsTab;
  setNav: (nav: NavKey) => void;
  setSettingsTab: (tab: SettingsTab) => void;

  // 后端健康
  health: HealthInfo | null;
  online: boolean;
  refreshHealth: () => Promise<void>;

  // 配置
  config: AppConfig | null;
  loadConfig: () => Promise<void>;
  saveConfig: (config: AppConfig) => Promise<void>;

  // 会话
  sessions: Session[];
  activeSessionId: string | null;
  loadSessions: () => Promise<void>;
  selectSession: (id: string | null) => Promise<void>;
  createSession: () => Promise<string | null>;
  renameSession: (id: string, title: string) => Promise<void>;
  removeSession: (id: string) => Promise<void>;

  // 聊天
  messages: Message[];
  streaming: boolean;
  streamingText: string;
  /** 流式期间正在执行的工具调用（结束即固化进 assistant 消息的 tool_calls） */
  streamingToolCalls: StreamingToolCall[];
  /** 当前会话选择的模型（providerId::modelId）与思考强度 */
  selectedModel: string | null;
  selectedThinking: string;
  setSelectedModel: (value: string | null) => void;
  setSelectedThinking: (value: string | null) => void;
  sendMessage: (text: string, opts?: { model?: string; thinking?: string }) => Promise<void>;
  stopStreaming: () => void;

  // 文件区
  workspaceDir: string;
  entries: FileEntry[];
  loadingFiles: boolean;
  previewFile: FileEntry | null;
  fileMaximized: boolean;
  navigateDir: (dir: string) => Promise<void>;
  openFile: (entry: FileEntry | null) => void;
  toggleFileMaximized: () => void;
}

export const useAppStore = create<AppStore>((set, get) => {
  /** 当前流式对话的取消句柄 */
  let streamController: { close: () => void } | null = null;

  return {
    activeNav: "chat",
    settingsTab: "appearance",
    setNav: (nav) => {
      set({ activeNav: nav });
      if (nav === "settings") set({ previewFile: null });
    },
    setSettingsTab: (tab) => set({ settingsTab: tab }),

    health: null,
    online: false,
    refreshHealth: async () => {
      try {
        const health = await api.health();
        const prev = get().health;
        // 仅当健康信息或在线状态变化时才更新，避免轮询导致整页无谓重渲染
        const changed =
          !get().online ||
          !prev ||
          prev.version !== health.version ||
          prev.status !== health.status ||
          prev.workingDir !== health.workingDir ||
          prev.providers !== health.providers ||
          prev.theme !== health.theme ||
          prev.lang !== health.lang;
        if (changed) set({ health, online: true });
      } catch {
        if (get().online) set({ online: false });
      }
    },

    config: null,
    loadConfig: async () => {
      try {
        const config = await api.getConfig();
        set({ config });
        // 语言以后端 config.toml 为准（桌面/网页一致），与 localStorage 不同时校正
        if (isLang(config.lang) && config.lang !== i18n.language) {
          applyLang(config.lang);
        }
        // 模型选择初始化/校正：无持久化选择时展示配置默认模型，
        // 持久化的选择已失效（提供商/模型被删）时回退默认。
        // 注意这里不写 localStorage —— 只有用户显式选择才持久化，
        // 避免 config 默认模型日后变更时被旧的持久化值压住。
        const [pid, mid] = (get().selectedModel ?? "").split("::");
        const valid = config.providers.some((p) => p.id === pid && p.models.includes(mid));
        if (!valid) {
          const fallback = defaultModelValue(config);
          if (fallback) set({ selectedModel: fallback });
        }
      } catch {
        /* 后端未就绪时保持现状 */
      }
    },
    saveConfig: async (config) => {
      await api.saveConfig(config);
      set({ config });
      await get().loadSessions();
    },

    sessions: [],
    activeSessionId: null,
    loadSessions: async () => {
      try {
        const sessions = await api.listSessions();
        set({ sessions });
      } catch {
        /* ignore */
      }
    },
    selectSession: async (id) => {
      // 停止进行中的流
      streamController?.close();
      set({ streaming: false, streamingText: "", activeSessionId: id, messages: [], previewFile: null });
      if (!id) return;
      try {
        const { messages } = await api.getSession(id);
        set({ messages });
      } catch {
        /* ignore */
      }
    },
    createSession: async () => {
      const { config } = get();
      const session = await api.createSession({
        provider_id: config?.default_provider,
        model: config?.default_model,
        // 默认标题跟随界面语言；后端将其视为"未命名"，首条消息后自动命名
        title: i18n.t("chat.newSession"),
      });
      await get().loadSessions();
      await get().selectSession(session.id);
      return session.id;
    },
    renameSession: async (id, title) => {
      await api.renameSession(id, title);
      set({
        sessions: get().sessions.map((s) => (s.id === id ? { ...s, title } : s)),
      });
    },
    removeSession: async (id) => {
      try {
        await api.deleteSession(id);
        if (get().activeSessionId === id) {
          set({ activeSessionId: null, messages: [] });
        }
        await get().loadSessions();
      } catch (err) {
        toast.error(i18n.t("sessionList.deleteFailed", { error: String(err) }));
      }
    },

    messages: [],
    streaming: false,
    streamingText: "",
    streamingToolCalls: [],
    selectedModel: localStorage.getItem("boenmind.selectedModel"),
    selectedThinking: "off",
    setSelectedModel: (value) => {
      // 刷新后保留上次选中的模型
      if (value) localStorage.setItem("boenmind.selectedModel", value);
      else localStorage.removeItem("boenmind.selectedModel");
      set({ selectedModel: value });
    },
    setSelectedThinking: (value) => set({ selectedThinking: value ?? "off" }),
    sendMessage: async (text, opts) => {
      const { activeSessionId } = get();
      let sessionId = activeSessionId;
      if (!sessionId) {
        sessionId = await get().createSession();
      }
      if (!sessionId) return;

      // 本地乐观追加用户消息
      const userMsg: Message = {
        id: Date.now(),
        session_id: sessionId,
        role: "user",
        content: text,
        created_at: Math.floor(Date.now() / 1000),
      };
      set((s) => ({ messages: [...s.messages, userMsg], streaming: true, streamingText: "", streamingToolCalls: [] }));

      const handleEvent = (ev: ChatStreamEvent) => {
        const s = get();
        switch (ev.type) {
          case "textDelta":
            set({ streamingText: s.streamingText + ev.delta });
            break;
          case "toolCallStart":
            set({
              streamingToolCalls: [
                ...s.streamingToolCalls,
                { id: ev.id, name: ev.name, args: ev.args, isError: false, done: false },
              ],
            });
            break;
          case "toolCallEnd":
            set({
              streamingToolCalls: s.streamingToolCalls.map((c) =>
                c.id === ev.id ? { ...c, isError: ev.isError, done: true } : c,
              ),
            });
            break;
          case "done": {
            const finalText = s.streamingText;
            // 流式工具调用固化为消息结构（seq 按顺序编号）
            const toolCalls: ToolCall[] = s.streamingToolCalls.map((c, i) => ({
              seq: i,
              tool_name: c.name,
              args: c.args,
              is_error: c.isError,
            }));
            const assistantMsg: Message = {
              id: Date.now(),
              session_id: sessionId!,
              role: "assistant",
              content: finalText,
              created_at: Math.floor(Date.now() / 1000),
              tool_calls: toolCalls,
            };
            set({ streaming: false, streamingText: "", streamingToolCalls: [], messages: [...s.messages, assistantMsg] });
            break;
          }
          case "error":
            set({ streaming: false, streamingToolCalls: [] });
            // 错误信息以用户可见的形式追加
            set((st) => ({
              messages: [
                ...st.messages,
                {
                  id: Date.now(),
                  session_id: sessionId!,
                  role: "assistant",
                  content: `⚠️ ${ev.message}`,
                  created_at: Math.floor(Date.now() / 1000),
                },
              ],
            }));
            break;
          default:
            break;
        }
      };

      const controller = api.chat(sessionId, text, handleEvent, {
        model: opts?.model ?? undefined,
        thinking: opts?.thinking ?? undefined,
      });
      streamController = controller;
      await controller.done;
      streamController = null;
      if (get().streaming) {
        // 异常结束（无 done 事件）时清理状态
        set({ streaming: false, streamingToolCalls: [] });
      }
      await get().loadSessions();
    },
    stopStreaming: () => {
      streamController?.close();
      set({ streaming: false, streamingToolCalls: [] });
    },

    workspaceDir: "",
    entries: [],
    loadingFiles: false,
    previewFile: null,
    fileMaximized: false,
    navigateDir: async (dir) => {
      set({ loadingFiles: true, workspaceDir: dir, previewFile: null });
      try {
        const { entries } = await api.listWorkspace(dir);
        set({ entries, loadingFiles: false });
      } catch {
        set({ entries: [], loadingFiles: false });
      }
    },
    openFile: (entry) => set({ previewFile: entry }),
    toggleFileMaximized: () => set((s) => ({ fileMaximized: !s.fileMaximized })),
  };
});
