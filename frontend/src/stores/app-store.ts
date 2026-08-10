/**
 * 全局状态：导航、会话、聊天流、文件区、后端健康状态。
 */
import { create } from "zustand";
import { api, type AppConfig, type ChatStreamEvent, type FileEntry, type HealthInfo, type Message, type Session } from "@/api/client";

export type NavKey = "chat" | "gallery" | "knowledge" | "settings";
export type SettingsTab = "appearance" | "providers" | "workspace" | "plugins" | "about";

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
          prev.theme !== health.theme;
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
      await api.deleteSession(id);
      if (get().activeSessionId === id) {
        set({ activeSessionId: null, messages: [] });
      }
      await get().loadSessions();
    },

    messages: [],
    streaming: false,
    streamingText: "",
    selectedModel: null,
    selectedThinking: "off",
    setSelectedModel: (value) => set({ selectedModel: value }),
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
      set((s) => ({ messages: [...s.messages, userMsg], streaming: true, streamingText: "" }));

      const handleEvent = (ev: ChatStreamEvent) => {
        const s = get();
        switch (ev.type) {
          case "textDelta":
            set({ streamingText: s.streamingText + ev.delta });
            break;
          case "done": {
            const finalText = s.streamingText;
            const assistantMsg: Message = {
              id: Date.now(),
              session_id: sessionId!,
              role: "assistant",
              content: finalText,
              created_at: Math.floor(Date.now() / 1000),
            };
            set({ streaming: false, streamingText: "", messages: [...s.messages, assistantMsg] });
            break;
          }
          case "error":
            set({ streaming: false });
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
        set({ streaming: false });
      }
      await get().loadSessions();
    },
    stopStreaming: () => {
      streamController?.close();
      set({ streaming: false });
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
