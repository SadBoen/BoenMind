/**
 * 全局状态：导航、会话、聊天流、文件区、后端健康状态。
 */
import { create } from "zustand";
import { api, type AppConfig, type ChatStreamEvent, type FileEntry, type HealthInfo, type Message, type Session, type Task, type TodoItem, type ToolCall } from "@/api/client";
import i18n, { applyLang, isLang } from "@/i18n";
import { toast } from "sonner";

// 导航/设置页类型定义在 lib/app-registry.tsx 注册表（单一数据源）
export type { SettingsTab, AppId } from "@/lib/app-registry";
import type { SettingsTab, AppId } from "@/lib/app-registry";
import { ACCENTS, applyAccent, applyReduceMotion, type Accent } from "@/lib/appearance";
import {
  applySkin,
  applySkinParams,
  loadSkinBackground,
  loadSkinEffect,
  loadSkinId,
  loadSkinParams,
  loadSkinWallpaper,
  saveSkinBackground,
  saveSkinEffect,
  saveSkinParams,
  saveSkinWallpaper,
  type SkinBackground,
} from "@/lib/skin";
import { type BackgroundEffectId } from "@/lib/skin";
import { type SkinId, type SkinParams } from "@/skins";

/** 活任务清单操作（M2；与后端 todo 工具参数对齐，index 1 起） */
export type TodoOp = {
  action: "add" | "update" | "remove" | "list";
  index?: number;
  content?: string;
  status?: string;
  priority?: string;
};

/** 流式中的工具调用（isError 未定前为执行中状态） */
interface StreamingToolCall {
  id: string;
  name: string;
  args: unknown;
  isError: boolean;
  /** 已收到 ToolCallEnd（执行结束，颜色取 isError；未结束时保持中性灰） */
  done: boolean;
}

/**
 * 编程应用项目（项目切换，2026-08-15）：根目录 + 显示名。
 * 前端项目集合 + 后端 workspace 路径参数化（root 缺省 = 配置工作目录兜底）。
 * localStorage 持久化（模式同 appSessionIds：手动读写，不走 zustand persist）。
 */
export interface Project {
  id: string;
  name: string;
  root: string;
}

const PROJECTS_KEY = "boenmind.projects";
const CURRENT_PROJECT_KEY = "boenmind.currentProject";
/** 现场恢复（P4，2026-08-15 长程测试）：刷新/重启后回到原应用与会话 */
const ACTIVE_NAV_KEY = "boenmind.activeNav";
const ACTIVE_SESSION_KEY = "boenmind.activeSessionId";

// 与注册表 APPS 的键集保持一致（清理轮 2026-08-16：plugins/steward 已随桌面壳收口）
const APP_IDS: AppId[] = ["chat", "coding", "wiki", "settings"];

function loadProjects(): Project[] {
  try {
    const raw = localStorage.getItem(PROJECTS_KEY);
    const arr = raw ? (JSON.parse(raw) as Project[]) : [];
    return Array.isArray(arr) ? arr : [];
  } catch {
    return [];
  }
}

function loadCurrentProjectId(projects: Project[]): string | null {
  const id = localStorage.getItem(CURRENT_PROJECT_KEY);
  return id && projects.some((p) => p.id === id) ? id : null;
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
  /** 经典界面的当前导航 */
  activeNav: AppId;
  setActiveNav: (id: AppId) => void;

  // 设置页（设置应用内部）
  settingsTab: SettingsTab;
  setSettingsTab: (tab: SettingsTab) => void;
  /** 设置分级（设置架构 §十）：basic = 普通用户（默认）；expert = 资深者（更多设置内容） */
  settingsTier: "basic" | "expert";
  setSettingsTier: (tier: "basic" | "expert") => void;
  /** 外观高级项（资深模式可见）：强调色 / 减少动画 */
  accent: Accent;
  setAccent: (accent: Accent) => void;
  reduceMotion: boolean;
  setReduceMotion: (enabled: boolean) => void;

  // 皮肤（外观：风格模板切换，2026-08-16；不改布局，只换材质风格）
  skin: SkinId;
  setSkin: (id: SkinId) => void;
  /** 当前皮肤参数（未设置的键走 SkinParam.default） */
  skinParams: SkinParams;
  setSkinParam: (key: string, value: number) => void;
  /** 背景图（data = 本地压缩 dataURL；url = 外链） */
  skinBackground: SkinBackground | null;
  setSkinBackground: (bg: SkinBackground | null) => void;
  /** 内置预设壁纸（preset id；与自定义背景图互斥） */
  skinWallpaper: string | null;
  setSkinWallpaper: (id: string | null) => void;
  /** 背景特效（独立于皮肤/壁纸：动画层叠加；wave=蓝色波纹，none=无） */
  backgroundEffect: BackgroundEffectId;
  setBackgroundEffect: (id: BackgroundEffectId) => void;
  /** 自动配色：上传/应用背景图时按图片调节色调/透明度/模糊 */
  skinAuto: boolean;
  setSkinAuto: (enabled: boolean) => void;

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
  /** 各场景最近使用的会话（一软件一会话，架构 §四·B 补充）：应用激活时恢复 */
  appSessionIds: Record<string, string>;
  loadSessions: () => Promise<void>;
  selectSession: (id: string | null) => Promise<void>;
  /** 记录某场景最近使用的会话（selectSession 内部调用；持久化跨刷新） */
  rememberAppSession: (app: string, sessionId: string) => void;
  createSession: (app?: string) => Promise<string | null>;
  /** 应用激活：把聚焦会话切到该场景最近使用的会话（无则保持 null，不自动创建） */
  activateApp: (app: string) => Promise<void>;
  /** 确保该场景有会话（无则创建）：编程壳对话 Tab 等需要真实会话的入口用 */
  ensureAppSession: (app: string) => Promise<string | null>;
  renameSession: (id: string, title: string) => Promise<void>;
  /** 会话级分叉（答复末尾分叉按钮）：返回新会话 id */
  forkFromMessage: (messageId: number) => Promise<string | null>;
  removeSession: (id: string) => Promise<void>;
  clearSessionEvents: (id: string) => Promise<void>;

  // 聊天
  messages: Message[];
  streaming: boolean;
  streamingText: string;
  /** 流式期间正在执行的工具调用（结束即固化进 assistant 消息的 tool_calls） */
  streamingToolCalls: StreamingToolCall[];
  /** 活跃任务心跳进度（SSE taskProgress 事件；null = 无进行中任务） */
  taskProgress: string | null;
  /** 最近一次任务记录（打开会话/流结束后刷新；断线续跑恢复展示用） */
  lastTask: Task | null;
  /** 状态栏 token 用量刷新信号：流结束（done/停止）时 +1，TokenUsage 订阅重拉 */
  usageVersion: number;
  /**
   * 聊天单元内嵌会话列表显隐（按场景：chat/coding…；默认 chat 展开、其余折叠）。
   * 用户拍板"列表在聊天单元内部，由顶部状态栏控制显隐"（2026-08-15）——
   * 状态栏 prefix 槽位的三横按钮 toggle 本标志，ChatPane 订阅渲染内嵌列表。
   */
  chatSessionsOpen: Record<string, boolean>;
  toggleChatSessions: (app: string) => void;
  /** 当前会话选择的模型（providerId::modelId）与思考强度 */
  selectedModel: string | null;
  selectedThinking: string;
  setSelectedModel: (value: string | null) => void;
  setSelectedThinking: (value: string | null) => void;
  sendMessage: (text: string, opts?: { provider?: string; model?: string; thinking?: string }) => Promise<void>;
  stopStreaming: () => void;

  // 插件权限
  /** 挂起的权限询问（SSE permissionRequest 事件触发弹窗；null = 无） */
  pendingPermission: {
    id: string;
    extensionId?: string;
    capability: string;
    message: string;
  } | null;
  /** 当前插件权限模式（default / safe / balanced / yolo） */
  permissionMode: string;
  /** 回传权限决策（allow + always；决策记忆在上游，见 permission.rs 注释） */
  respondPermission: (allow: boolean, always: boolean) => Promise<void>;
  /** 询问超时（60s）自动关闭弹窗（后端已 fail-closed） */
  dismissPermission: () => void;
  /** 读取权限模式（聊天工具条与设置页共用） */
  loadPermissionMode: () => Promise<void>;
  /** 切换权限模式（yolo = permissive + allowDangerous） */
  setPermissionMode: (mode: string) => Promise<void>;

  // 编程项目（项目切换：前端项目集合 + 后端 workspace root 参数化）
  projects: Project[];
  /** 当前项目 id（null = 未选项目，workspace 走配置工作目录兜底） */
  currentProjectId: string | null;
  /** 当前项目对象（派生；null = 配置工作目录兜底） */
  currentProject: Project | null;
  addProject: (name: string, root: string) => void;
  removeProject: (id: string) => void;
  /** 切换项目：文件树/预览状态清空回新项目根；GitBar/分支图经 currentProjectId 订阅刷新 */
  selectProject: (id: string) => void;

  // 文件区
  workspaceDir: string;
  entries: FileEntry[];
  loadingFiles: boolean;
  previewFile: FileEntry | null;
  fileMaximized: boolean;
  navigateDir: (dir: string) => Promise<void>;
  openFile: (entry: FileEntry | null) => void;
  toggleFileMaximized: () => void;
  /** 只刷新当前目录列表（不动 previewFile——编辑器保存后刷新用） */
  refreshFiles: () => Promise<void>;

  // 活任务清单（M2：todo/write 事件投影——事件流是权威，store 只是投影）
  todos: TodoItem[];
  /** 事件流投喂（subscribeEvents 的 onEvent 直接调；重放+实时同路） */
  setTodosFromEvent: (todos: TodoItem[]) => void;
  /** 手动操作（面板增删改；调 REST 面，服务端落事件快照后返回最新清单） */
  applyTodoOp: (op: TodoOp) => Promise<void>;
}

export const useAppStore = create<AppStore>((set, get) => {
  /** 当前流式对话的取消句柄 */
  let streamController: { close: () => void } | null = null;

  /** 编程项目初始态（localStorage 恢复一次） */
  const initialProjects = loadProjects();
  const initialCurrentProjectId = loadCurrentProjectId(initialProjects);

  /** 固化流式内容为助手消息（done 事件与停止兜底共用） */
  const finalizeStream = (sessionId: string) => {
    const s = get();
    const finalText = s.streamingText;
    // 流式工具调用固化为消息结构（seq 按顺序编号）
    const toolCalls: ToolCall[] = s.streamingToolCalls.map((c, i) => ({
      seq: i,
      tool_name: c.name,
      args: c.args,
      is_error: c.isError,
    }));
    set({
      streaming: false,
      streamingText: "",
      streamingToolCalls: [],
      // 状态栏 token 用量刷新信号（流结束即 +1，TokenUsage 订阅重新拉取）
      usageVersion: s.usageVersion + 1,
      messages: [
        ...s.messages,
        {
          id: Date.now(),
          session_id: sessionId,
          role: "assistant",
          content: finalText,
          created_at: Math.floor(Date.now() / 1000),
          tool_calls: toolCalls,
        },
      ],
    });
    refreshLastTask(sessionId);
  };

  /** 任务终态刷新：流结束后拉取最近任务记录（清心跳、展示终态/进度） */
  const refreshLastTask = (sessionId: string) => {
    void api
      .listSessionTasks(sessionId)
      .then((tasks) => set({ lastTask: tasks[0] ?? null, taskProgress: null }))
      .catch(() => set({ taskProgress: null }));
  };

  return {
    // 默认经典软件界面（用户拍板）；桌面形态退役（2026-08-16），开关仅占位
    activeNav: (APP_IDS.find((a) => a === localStorage.getItem(ACTIVE_NAV_KEY)) ?? "chat") as AppId,
    setActiveNav: (id) => {
      localStorage.setItem(ACTIVE_NAV_KEY, id);
      set({ activeNav: id });
    },

    settingsTab: "appearance",
    setSettingsTab: (tab) => set({ settingsTab: tab }),
    // 设置分级：basic（默认）/ expert；切换只改可见性，不动任何设置值
    settingsTier: (localStorage.getItem("boenmind.settingsTier") === "expert" ? "expert" : "basic"),
    setSettingsTier: (tier) => {
      localStorage.setItem("boenmind.settingsTier", tier);
      set({ settingsTier: tier });
    },
    accent: (() => {
      const saved = localStorage.getItem("boenmind.appearance.accent");
      return (ACCENTS.find((a) => a.key === saved)?.key ?? "default") as Accent;
    })(),
    setAccent: (accent) => {
      localStorage.setItem("boenmind.appearance.accent", accent);
      applyAccent(accent);
      set({ accent });
    },
    reduceMotion: localStorage.getItem("boenmind.appearance.reduceMotion") === "1",
    setReduceMotion: (enabled) => {
      localStorage.setItem("boenmind.appearance.reduceMotion", enabled ? "1" : "0");
      applyReduceMotion(enabled);
      set({ reduceMotion: enabled });
    },

    // 皮肤：初始从 localStorage 恢复并立即应用（参数按皮肤分开持久化）
    skin: (() => {
      const id = loadSkinId();
      applySkin(id, loadSkinParams(id));
      return id;
    })(),
    setSkin: (skin) => {
      localStorage.setItem("boenmind.skin", skin);
      applySkin(skin, loadSkinParams(skin));
      set({ skin, skinParams: loadSkinParams(skin) });
    },
    skinParams: loadSkinParams(loadSkinId()),
    setSkinParam: (key, value) => {
      const skin = get().skin;
      const skinParams = { ...get().skinParams, [key]: value };
      saveSkinParams(skin, skinParams);
      applySkinParams(skin, skinParams);
      set({ skinParams });
    },
    skinBackground: loadSkinBackground(),
    setSkinBackground: (skinBackground) => {
      saveSkinBackground(skinBackground);
      // 自定义背景图与预设壁纸互斥（应用自定义时清预设）
      if (skinBackground) {
        saveSkinWallpaper(null);
        set({ skinBackground, skinWallpaper: null });
      } else {
        set({ skinBackground });
      }
    },
    skinWallpaper: loadSkinWallpaper(),
    setSkinWallpaper: (skinWallpaper) => {
      saveSkinWallpaper(skinWallpaper);
      // 预设壁纸与自定义背景图互斥（选预设时清自定义）
      if (skinWallpaper) {
        saveSkinBackground(null);
        set({ skinWallpaper, skinBackground: null });
      } else {
        set({ skinWallpaper });
      }
    },
    backgroundEffect: loadSkinEffect(),
    setBackgroundEffect: (backgroundEffect) => {
      saveSkinEffect(backgroundEffect);
      set({ backgroundEffect });
    },
    skinAuto: localStorage.getItem("boenmind.skin.auto") === "1",
    setSkinAuto: (enabled) => {
      localStorage.setItem("boenmind.skin.auto", enabled ? "1" : "0");
      set({ skinAuto: enabled });
    },

    pendingPermission: null,
    permissionMode: "default",
    respondPermission: async (allow, always) => {
      const pending = get().pendingPermission;
      if (!pending) return;
      try {
        await api.respondPermission(pending.id, allow, always);
      } finally {
        set({ pendingPermission: null });
      }
    },
    // 询问超时（60s）自动关闭弹窗；后端已 fail-closed 拒绝，前端无需上报
    dismissPermission: () => set({ pendingPermission: null }),
    loadPermissionMode: async () => {
      try {
        const cfg = await api.getConfig();
        const policy = cfg.extension_policy;
        const mode =
          policy === "permissive" && cfg.extension_allow_dangerous
            ? "yolo"
            : policy === "safe" || policy === "balanced"
              ? policy
              : "default";
        set({ permissionMode: mode });
      } catch {
        // 读取失败保持默认展示
      }
    },
    setPermissionMode: async (mode) => {
      const prev = get().permissionMode;
      set({ permissionMode: mode });
      try {
        const cfg = await api.getConfig();
        cfg.extension_policy =
          mode === "yolo" ? "permissive" : mode === "default" ? undefined : mode;
        cfg.extension_allow_dangerous = mode === "yolo";
        await api.saveConfig(cfg);
      } catch (err) {
        set({ permissionMode: prev });
        throw err;
      }
    },

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
        // 恢复连接：关闭固定断连提示（若之前弹出过）
        toast.dismiss("backend-offline");
      } catch {
        if (get().online) {
          set({ online: false });
          // 断连提示（2026-08-16 用户要求"别一直弹"）：固定 id + duration Infinity =
          // 只弹一个且不消失，恢复连接后 dismiss；状态栏红字常驻兜底
          toast.error(i18n.t("statusbar.backendDisconnected"), {
            id: "backend-offline",
            duration: Infinity,
          });
        }
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
    activeSessionId: localStorage.getItem(ACTIVE_SESSION_KEY),
    appSessionIds: JSON.parse(localStorage.getItem("boenmind.appSessionIds") ?? "{}") as Record<string, string>,
    loadSessions: async () => {
      try {
        const sessions = await api.listSessions();
        set({ sessions });
        // 现场恢复校验：持久化的 activeSessionId 已不存在（被删/数据目录
        // 切换）→ 放弃聚焦，避免打开会话报"会话不存在"；或所属应用与
        // 恢复的导航不一致（跨应用错配，如 coding 视图挂 chat 会话）→
        // 回退到该应用记住的会话（appSessionIds 按应用持久化，更可靠）
        const restored = get().activeSessionId;
        if (restored) {
          const s = sessions.find((x) => x.id === restored);
          if (!s) {
            localStorage.removeItem(ACTIVE_SESSION_KEY);
            set({ activeSessionId: null, messages: [] });
          } else if (s.app !== get().activeNav) {
            const fallback = get().appSessionIds[get().activeNav] ?? null;
            if (fallback) {
              localStorage.setItem(ACTIVE_SESSION_KEY, fallback);
              set({ activeSessionId: fallback, messages: [] });
            } else {
              localStorage.removeItem(ACTIVE_SESSION_KEY);
              set({ activeSessionId: null, messages: [] });
            }
          }
        }
        // 场景记录清理：引用已不存在的会话（被删/数据目录切换）时移除，
        // 避免恢复现场时 selectSession 到失效 id（后端报"会话不存在"）
        const appSessionIds = { ...get().appSessionIds };
        let dirty = false;
        for (const [app, sid] of Object.entries(appSessionIds)) {
          if (!sessions.some((s) => s.id === sid)) {
            delete appSessionIds[app];
            dirty = true;
          }
        }
        if (dirty) {
          localStorage.setItem("boenmind.appSessionIds", JSON.stringify(appSessionIds));
          set({ appSessionIds });
        }
      } catch {
        /* ignore */
      }
    },
    /** 记录某场景最近使用的会话（一软件一会话：应用激活时恢复现场） */
    rememberAppSession: (app: string, sessionId: string) => {
      const appSessionIds = { ...get().appSessionIds, [app]: sessionId };
      localStorage.setItem("boenmind.appSessionIds", JSON.stringify(appSessionIds));
      set({ appSessionIds });
    },
    selectSession: async (id) => {
      // 停止进行中的流
      streamController?.close();
      set({ streaming: false, streamingText: "", activeSessionId: id, messages: [], previewFile: null });
      // 现场恢复：聚焦会话持久化（刷新/重启后回到同一会话）
      if (id) {
        localStorage.setItem(ACTIVE_SESSION_KEY, id);
      } else {
        localStorage.removeItem(ACTIVE_SESSION_KEY);
      }
      if (!id) return;
      // 记录该会话所属场景（loadSessions 后 sessions 含 app 字段）
      const scene = get().sessions.find((s) => s.id === id)?.app;
      if (scene) get().rememberAppSession(scene, id);
      try {
        const { messages } = await api.getSession(id);
        set({ messages });
      } catch {
        // 会话已不存在（被删/数据目录切换）：放弃聚焦并清理场景记录，
        // 避免残留失效 id（下次恢复现场仍指向它）
        const appSessionIds = { ...get().appSessionIds };
        let dirty = false;
        for (const [app, sid] of Object.entries(appSessionIds)) {
          if (sid === id) {
            delete appSessionIds[app];
            dirty = true;
          }
        }
        if (dirty) {
          localStorage.setItem("boenmind.appSessionIds", JSON.stringify(appSessionIds));
          set({ appSessionIds });
        }
        set({ activeSessionId: null, messages: [] });
      }
      // 断线续跑恢复：拉取最近任务（含心跳进度/终态，见任务状态条）
      try {
        const tasks = await api.listSessionTasks(id);
        set({ lastTask: tasks[0] ?? null });
      } catch {
        /* ignore */
      }
    },
    createSession: async (app = "chat") => {
      const { config } = get();
      const session = await api.createSession({
        provider_id: config?.default_provider,
        model: config?.default_model,
        // 默认标题跟随界面语言；后端将其视为"未命名"，首条消息后自动命名
        title: i18n.t("chat.newSession"),
        app,
      });
      await get().loadSessions();
      await get().selectSession(session.id);
      return session.id;
    },
    activateApp: async (app) => {
      const { sessions, appSessionIds, activeSessionId } = get();
      // 聚焦会话已属于该场景 → 不动（编程壳内部 Tab 切换也走这里）
      if (sessions.find((s) => s.id === activeSessionId)?.app === app) return;
      const id = appSessionIds[app] ?? sessions.find((s) => s.app === app)?.id;
      if (id) {
        await get().selectSession(id);
      } else {
        // 无该场景会话：清掉聚焦会话（聚焦会话永远属于当前聚焦应用，
        // 编程壳任务清单等投影组件才不会订阅到别的场景的会话）
        streamController?.close();
        localStorage.removeItem(ACTIVE_SESSION_KEY);
        set({
          streaming: false,
          streamingText: "",
          activeSessionId: null,
          messages: [],
          previewFile: null,
          lastTask: null,
        });
      }
    },
    ensureAppSession: async (app) => {
      const { sessions, appSessionIds, activeSessionId } = get();
      if (sessions.find((s) => s.id === activeSessionId)?.app === app) return activeSessionId;
      const id = appSessionIds[app] ?? sessions.find((s) => s.app === app)?.id;
      if (id) {
        await get().selectSession(id);
        return id;
      }
      return get().createSession(app);
    },
    renameSession: async (id, title) => {
      await api.renameSession(id, title);
      set({
        sessions: get().sessions.map((s) => (s.id === id ? { ...s, title } : s)),
      });
    },
    /** 会话级分叉（2026-08-16 用户定调"答复末尾分叉"）：新会话复制历史到
     * 该消息，然后切到新会话。 */
    forkFromMessage: async (messageId: number) => {
      const srcId = get().activeSessionId;
      if (!srcId) return null;
      const session = await api.forkSession(srcId, messageId);
      await get().loadSessions();
      await get().selectSession(session.id);
      return session.id;
    },
    removeSession: async (id) => {
      try {
        await api.deleteSession(id);
        if (get().activeSessionId === id) {
          localStorage.removeItem(ACTIVE_SESSION_KEY);
          set({ activeSessionId: null, messages: [] });
        }
        // 清理场景记录（该场景无会话时 activateApp 保持 null，不残留死引用）
        const appSessionIds = { ...get().appSessionIds };
        for (const [app, sid] of Object.entries(appSessionIds)) {
          if (sid === id) delete appSessionIds[app];
        }
        localStorage.setItem("boenmind.appSessionIds", JSON.stringify(appSessionIds));
        set({ appSessionIds });
        await get().loadSessions();
      } catch (err) {
        toast.error(i18n.t("sessionList.deleteFailed", { error: String(err) }));
      }
    },
    // 清空会话事件日志（回收站 C2：用户主动清除；会话与消息仍在）
    clearSessionEvents: async (id) => {
      try {
        await api.clearSessionEvents(id);
        toast.success(i18n.t("sessionList.clearEventsDone"));
      } catch (err) {
        toast.error(i18n.t("sessionList.clearEventsFailed", { error: String(err) }));
      }
    },

    messages: [],
    streaming: false,
    streamingText: "",
    streamingToolCalls: [],
    taskProgress: null,
    lastTask: null,
    usageVersion: 0,
    // 聊天单元内嵌列表默认态：聊天应用展开（会话列表是聊天单元的一部分），
    // 编程等场景折叠（右列空间有限，随时经状态栏三横展开）
    chatSessionsOpen: { chat: true },
    toggleChatSessions: (app) =>
      set((s) => ({
        chatSessionsOpen: { ...s.chatSessionsOpen, [app]: !(s.chatSessionsOpen[app] ?? false) },
      })),
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
          case "permissionRequest":
            // 同一时间只展示一个询问：新请求覆盖旧的（旧的会超时 fail-closed）
            set({
              pendingPermission: {
                id: ev.id,
                extensionId: ev.extensionId,
                capability: ev.capability,
                message: ev.message,
              },
            });
            break;
          case "taskProgress":
            // 任务心跳：进行中任务的状态条展示（断线重连后从 listSessionTasks 恢复）
            set({ taskProgress: ev.progress });
            break;
          case "done": {
            // 后端确认结束（正常完成或停止后的部分文本）：固化流式内容
            finalizeStream(sessionId!);
            // 重拉消息使 id 与服务器一致（后端落库先于 done 事件）。
            // 否则乐观 id（Date.now()）会泄漏进 fork 契约——后端按自增 id 定位，
            // 时间戳恒大于任何自增 id，导致分叉静默复制整个会话。
            void api.getSession(sessionId!).then(({ messages }) => set({ messages }));
            break;
          }
          case "error":
            set({ streaming: false, streamingText: "", streamingToolCalls: [] });
            refreshLastTask(sessionId!);
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
        provider: opts?.provider ?? undefined,
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
      const { activeSessionId } = get();
      const controller = streamController;
      if (!controller) return;
      if (activeSessionId) {
        // 请求后端取消 prompt：不主动断开 SSE，等后端 abort 后下发的 done 事件
        // 固化已生成的部分内容（与正常完成走同一条 finalize 路径）
        void api.stopChat(activeSessionId).catch(() => {});
        // 兜底：后端 8 秒内未收口（网络异常/后端无响应）时本地固化并断开
        setTimeout(() => {
          if (get().streaming && streamController === controller) {
            finalizeStream(activeSessionId);
          }
        }, 8000);
      } else {
        controller.close();
        set({ streaming: false, streamingToolCalls: [] });
      }
    },

    projects: initialProjects,
    currentProjectId: initialCurrentProjectId,
    currentProject: initialProjects.find((p) => p.id === initialCurrentProjectId) ?? null,
    addProject: (name, root) => {
      const project: Project = { id: crypto.randomUUID(), name: name.trim() || root, root: root.trim() };
      set((s) => {
        const projects = [...s.projects, project];
        localStorage.setItem(PROJECTS_KEY, JSON.stringify(projects));
        // 首个项目自动设为当前（后续新建不抢焦点）
        const currentProjectId = s.currentProjectId ?? project.id;
        localStorage.setItem(CURRENT_PROJECT_KEY, currentProjectId);
        return { projects, currentProjectId, currentProject: projects.find((p) => p.id === currentProjectId) ?? null };
      });
    },
    removeProject: (id) => {
      set((s) => {
        const projects = s.projects.filter((p) => p.id !== id);
        localStorage.setItem(PROJECTS_KEY, JSON.stringify(projects));
        // 删除的是当前项目 → 回退到列表首个（无则 null = 配置工作目录兜底）
        const currentProjectId = s.currentProjectId === id ? (projects[0]?.id ?? null) : s.currentProjectId;
        localStorage.setItem(CURRENT_PROJECT_KEY, currentProjectId ?? "");
        return {
          projects,
          currentProjectId,
          currentProject: projects.find((p) => p.id === currentProjectId) ?? null,
          workspaceDir: "",
          entries: [],
          previewFile: null,
        };
      });
    },
    selectProject: (id) => {
      set((s) => {
        if (!s.projects.some((p) => p.id === id) || s.currentProjectId === id) return s;
        localStorage.setItem(CURRENT_PROJECT_KEY, id);
        // 文件树回新项目根、清空预览（编辑器内容属于旧项目，不跨项目保留）
        return {
          currentProjectId: id,
          currentProject: s.projects.find((p) => p.id === id) ?? null,
          workspaceDir: "",
          entries: [],
          previewFile: null,
        };
      });
    },

    workspaceDir: "",
    entries: [],
    loadingFiles: false,
    previewFile: null,
    fileMaximized: false,
    navigateDir: async (dir) => {
      set({ loadingFiles: true, workspaceDir: dir, previewFile: null });
      try {
        const root = get().currentProject?.root;
        const { entries } = await api.listWorkspace(dir, root);
        set({ entries, loadingFiles: false });
      } catch {
        set({ entries: [], loadingFiles: false });
      }
    },
    openFile: (entry) => set({ previewFile: entry }),
    toggleFileMaximized: () => set((s) => ({ fileMaximized: !s.fileMaximized })),
    refreshFiles: async () => {
      const dir = get().workspaceDir;
      const root = get().currentProject?.root;
      set({ loadingFiles: true });
      try {
        const { entries } = await api.listWorkspace(dir, root);
        set({ entries, loadingFiles: false });
      } catch {
        set({ loadingFiles: false });
      }
    },

    todos: [],
    setTodosFromEvent: (todos) => set({ todos }),
    applyTodoOp: async (op) => {
      const sid = get().activeSessionId;
      if (!sid) {
        toast.error(i18n.t("coding.todos.noSession"));
        return;
      }
      try {
        const { todos } = await api.applyTodoOp(sid, op);
        set({ todos });
      } catch (err) {
        toast.error(String(err));
      }
    },
  };
});
