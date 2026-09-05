// W2:三栏布局 + 可拖分隔条(会话列表↕对话区、对话区↕工作区;宽度持久化
// localStorage,刷新保持)+ 设置中心入口(W2 整页式)。对话区 = W1 原样。
// W3:主题系统启动恢复(useThemeBoot)+ 玻璃主题花瓣层(Petals)。
import { BoenmindRuntimeProvider } from "./w1/runtime";
import { Thread } from "./w1/thread";
import { SettingsPage } from "./w2/SettingsPage";
import { WorkspaceFiles } from "./w2/WorkspaceFiles";
import { MusicPlayer } from "./w2/MusicPlayer";
import { Petals } from "./w3/Petals";
import {
  useThemeBoot,
  loadThemeState,
  applyTheme,
  type ThemeDef,
} from "./w3/themes";
import { useCallback, useEffect, useRef, useState } from "react";
import { storage, STORAGE_KEYS, sessionsStore, type SessionItemMeta } from "@/lib/storage";

type ThemeId = ThemeDef["id"];
import {
  MessageCircle,
  Music,
  Settings,
  Plus,
  RefreshCw,
  Trash2,
  MessageSquare,
} from "lucide-react";
import { BM_EVENTS, emit } from "./lib/bus";

// 三栏宽度持久化(W2 验收门 4:刷新后布局保持);W7 反馈:左右栏抽屉收放状态同库持久化
type Layout = {
  sessions: number;
  workspace: number;
  sessionsCollapsed: boolean;
  workspaceCollapsed: boolean;
};
const DEFAULT_LAYOUT: Layout = {
  sessions: 260,
  workspace: 320,
  sessionsCollapsed: false,
  workspaceCollapsed: false,
};
const LIMITS = {
  sessions: { min: 180, max: 440 },
  workspace: { min: 240, max: 560 },
};

function loadLayout(): Layout {
  try {
    const raw = storage.get(STORAGE_KEYS.LAYOUT);
    if (!raw) return DEFAULT_LAYOUT;
    const v = JSON.parse(raw) as Partial<Layout>;
    return {
      sessions: clamp(v.sessions ?? DEFAULT_LAYOUT.sessions, LIMITS.sessions),
      workspace: clamp(
        v.workspace ?? DEFAULT_LAYOUT.workspace,
        LIMITS.workspace,
      ),
      sessionsCollapsed: v.sessionsCollapsed ?? false,
      workspaceCollapsed: v.workspaceCollapsed ?? false,
    };
  } catch {
    return DEFAULT_LAYOUT;
  }
}

function clamp(v: number, l: { min: number; max: number }) {
  return Math.min(l.max, Math.max(l.min, v));
}

export default function App() {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [layout, setLayout] = useState<Layout>(loadLayout);
  useThemeBoot(); // W3:重启保持(localStorage 恢复主题+设置项)
  // 主题跟随 html[data-theme](外观页切换即时生效,花瓣层随之挂/卸)
  const [theme, setTheme] = useState(loadThemeState().theme);
  useEffect(() => {
    const ob = new MutationObserver(() =>
      setTheme(document.documentElement.getAttribute("data-theme") as ThemeId),
    );
    ob.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
    return () => ob.disconnect();
  }, []);

  const saveLayout = useCallback((next: Layout) => {
    setLayout(next);
    storage.set(STORAGE_KEYS.LAYOUT, JSON.stringify(next));
  }, []);

  const togglePanel = useCallback(
    (side: "sessions" | "workspace") => {
      setLayout((cur) => {
        const key = side === "sessions" ? "sessionsCollapsed" : "workspaceCollapsed";
        const next = { ...cur, [key]: !cur[key] } as Layout;
        storage.set(STORAGE_KEYS.LAYOUT, JSON.stringify(next));
        return next;
      });
    },
    [],
  );

  // 收起时该栏与其分隔条列宽归 0(children 数与模板列数保持一致)
  const s = layout.sessionsCollapsed ? "0px" : `${layout.sessions}px`;
  const ss = layout.sessionsCollapsed ? "0px" : "5px";
  const w = layout.workspaceCollapsed ? "0px" : `${layout.workspace}px`;
  const ws = layout.workspaceCollapsed ? "0px" : "5px";

  return (
    <BoenmindRuntimeProvider>
      <div
        className="app"
        style={{
          // 子元素顺序:rail, sessions, splitter, thread, splitter, workspace
          // 设置打开=聊天界面关闭(用户裁定):只留图标栏;composer 输入存于
          // runtime(Provider 级),设置关闭后重新挂载即恢复,内容不丢
          gridTemplateColumns: settingsOpen
            ? "52px minmax(0, 1fr)"
            : `52px ${s} ${ss} minmax(0, 1fr) ${ws} ${w}`,
        }}
      >
        {theme === "glass" ? <Petals /> : null}
        <Rail
          settingsActive={settingsOpen}
          onSettings={() => setSettingsOpen((v) => !v)}
          onNavigateChat={() => setSettingsOpen(false)}
          onNavigateApp={(app) => {
            setSettingsOpen(false);
            if (app === "music") {
              // 确保工作区面板展开，并切换至音乐播放器
              if (layout.workspaceCollapsed) {
                setLayout((cur) => {
                  const next = { ...cur, workspaceCollapsed: false };
                  storage.set(STORAGE_KEYS.LAYOUT, JSON.stringify(next));
                  return next;
                });
              }
              setTimeout(() => {
                emit(BM_EVENTS.openMusic);
              }, 10);
            }
          }}
        />
        {settingsOpen ? (
          <SettingsPage onClose={() => setSettingsOpen(false)} />
        ) : (
          <>
            <SessionPanel collapsed={layout.sessionsCollapsed} />
            {/* 分隔条必须始终渲染(收起时仅视觉隐藏)——网格子元素数与
                模板列数一一对应,否则整体错位一格(2026-09-02 修) */}
            <HSplitter
              hidden={layout.sessionsCollapsed}
              onDrag={(dx) =>
                saveLayout({
                  ...layout,
                  sessions: clamp(layout.sessions + dx, LIMITS.sessions),
                })
              }
            />
            <Thread
              sessionsCollapsed={layout.sessionsCollapsed}
              workspaceCollapsed={layout.workspaceCollapsed}
              onToggleSessions={() => togglePanel("sessions")}
              onToggleWorkspace={() => togglePanel("workspace")}
            />
            <HSplitter
              hidden={layout.workspaceCollapsed}
              onDrag={(dx) =>
                saveLayout({
                  ...layout,
                  workspace: clamp(layout.workspace - dx, LIMITS.workspace),
                })
              }
            />
            <WorkspacePanel collapsed={layout.workspaceCollapsed} />
          </>
        )}
      </div>
    </BoenmindRuntimeProvider>
  );
}

// 拖宽分隔条:纯前端(pointer 事件),拖动期间直接改列宽。
// hidden = 相邻面板收起:仍占位网格列(0px)但不可见不可交互——绝不条件
// 卸载,否则网格子元素与模板列错位,整个布局散架(2026-09-02 修)。
function HSplitter({
  onDrag,
  hidden,
}: {
  onDrag: (dx: number) => void;
  hidden?: boolean;
}) {
  const lastX = useRef<number | null>(null);

  const onPointerDown = (e: React.PointerEvent) => {
    lastX.current = e.clientX;
    e.currentTarget.setPointerCapture(e.pointerId);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (lastX.current === null) return;
    const dx = e.clientX - lastX.current;
    lastX.current = e.clientX;
    onDrag(dx);
  };
  const onPointerUp = (e: React.PointerEvent) => {
    lastX.current = null;
    e.currentTarget.releasePointerCapture(e.pointerId);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  };

  return (
    <div
      className="splitter"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      data-slot="splitter"
      title={hidden ? undefined : "拖动调整列宽"}
      style={hidden ? { visibility: "hidden", pointerEvents: "none" } : undefined}
    />
  );
}

function Rail({
  settingsActive,
  onSettings,
  onNavigateChat,
  onNavigateApp,
}: {
  settingsActive: boolean;
  onSettings: () => void;
  onNavigateChat: () => void;
  onNavigateApp: (app: "music") => void;
}) {
  // 图标栏只放真实可用的入口;未实现的功能不挂占位图标(风格统一轮裁定)
  return (
    <div className="rail">
      <button
        className={"rail-btn" + (!settingsActive ? " active" : "")}
        title="聊天"
        onClick={onNavigateChat}
      >
        <MessageCircle size={18} />
      </button>
      <button
        className="rail-btn"
        title="音乐播放器"
        onClick={() => onNavigateApp("music")}
      >
        <Music size={18} />
      </button>
      <div className="rail-spacer" />
      <button
        className={"rail-btn" + (settingsActive ? " active" : "")}
        title={settingsActive ? "关闭设置" : "设置"}
        onClick={onSettings}
        data-slot="open-settings"
      >
        <Settings size={18} />
      </button>
    </div>
  );
}

function SessionPanel({ collapsed }: { collapsed: boolean }) {
  const [flash, setFlash] = useState(false);
  const [sessions, setSessions] = useState<SessionItemMeta[]>(() => sessionsStore.list());
  const [activeSid, setActiveSid] = useState<string | null>(() => storage.get(STORAGE_KEYS.SESSION));

  const loadSessions = useCallback(() => {
    const list = sessionsStore.list();
    setSessions(list);
    setActiveSid(storage.get(STORAGE_KEYS.SESSION));
  }, []);

  useEffect(() => {
    loadSessions();
    const handleNewChat = () => {
      setFlash(true);
      setActiveSid(null);
      const timer = setTimeout(() => setFlash(false), 400);
      return () => clearTimeout(timer);
    };
    window.addEventListener(BM_EVENTS.chatNew, handleNewChat);
    window.addEventListener(BM_EVENTS.sessionsUpdated, loadSessions);
    return () => {
      window.removeEventListener(BM_EVENTS.chatNew, handleNewChat);
      window.removeEventListener(BM_EVENTS.sessionsUpdated, loadSessions);
    };
  }, [loadSessions]);

  const handleSelectSession = (sid: string) => {
    storage.set(STORAGE_KEYS.SESSION, sid);
    setActiveSid(sid);
    emit(BM_EVENTS.sessionSwitched, { sid });
  };

  const handleDeleteSession = (e: React.MouseEvent, sid: string) => {
    e.stopPropagation();
    const next = sessionsStore.remove(sid);
    setSessions(next);
    if (activeSid === sid) {
      // 若删除的是当前会话，则开辟新对话
      emit(BM_EVENTS.chatNew);
    }
  };

  return (
    <div
      className="sessions"
      style={{
        overflow: "hidden",
        minWidth: 0,
        ...(collapsed ? { padding: 0 } : null),
      }}
      aria-hidden={collapsed || undefined}
    >
      <div className="sessions-head">
        <span className="title">聊天</span>
        {/* 新建对话:清空聊天视图+丢弃会话号(下一条消息自动开新会话) */}
        <button
          className="icon-chip"
          title="新建对话"
          data-slot="new-chat"
          onClick={() => emit(BM_EVENTS.chatNew)}
        >
          <Plus size={16} />
        </button>
      </div>

      <div className="sessions-list" style={{ display: "flex", flexDirection: "column", gap: "6px", overflowY: "auto" }}>
        {/* 若当前处于新建状态(无 activeSid)，或者列表为空，显示当前新对话就绪卡片 */}
        {activeSid === null || sessions.length === 0 ? (
          <div className={"session-item active" + (flash ? " flash" : "")} data-slot="session-active-item">
            <div className="session-item-row">
              <MessageSquare size={13} className="shrink-0 text-primary opacity-80" />
              <span className="name">新对话</span>
            </div>
            <div className="status-hint">
              <span>● 就绪 (发送首条消息建库)</span>
            </div>
          </div>
        ) : null}

        {/* 历史多会话列表 */}
        {sessions.map((s) => {
          const isActive = s.id === activeSid;
          return (
            <div
              key={s.id}
              className={"session-item" + (isActive ? " active" : "")}
              onClick={() => handleSelectSession(s.id)}
              title={s.title}
              data-slot="session-item"
              data-sid={s.id}
            >
              <div className="session-item-row">
                <MessageSquare size={13} className="shrink-0 text-muted-foreground" />
                <span className="name">{s.title || "BoenMind 对话"}</span>
                <button
                  className="icon-chip delete-btn"
                  title="删除此会话"
                  onClick={(e) => handleDeleteSession(e, s.id)}
                  style={{ width: "20px", height: "20px", padding: 0 }}
                >
                  <Trash2 size={12} />
                </button>
              </div>
              <div className="status-hint">
                <span className="font-mono text-[10.5px] opacity-70">
                  {new Date(s.updatedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
                </span>
                {isActive ? <span className="text-primary font-medium">当前活动</span> : null}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function WorkspacePanel({ collapsed }: { collapsed: boolean }) {
  // 页签只放已交付能力(文件/音乐);产物/待办待实现后再上,不挂空占位
  const [tab, setTab] = useState<"files" | "music">("files");

  useEffect(() => {
    const handleOpenMusic = () => setTab("music");
    window.addEventListener(BM_EVENTS.openMusic, handleOpenMusic);
    return () => window.removeEventListener(BM_EVENTS.openMusic, handleOpenMusic);
  }, []);

  return (
    <div
      className="workspace"
      style={{
        overflow: "hidden",
        minWidth: 0,
        ...(collapsed ? { padding: 0 } : null),
      }}
      aria-hidden={collapsed || undefined}
    >
      <div className="workspace-head">
        <span className="title">WORKSPACE</span>
        <span className="actions">
          <button className="icon-chip" title="同步(重载目录树)"
            onClick={() => emit(BM_EVENTS.wsRefresh)}
          >
            <RefreshCw size={14} />
          </button>
        </span>
      </div>
      <div className="ws-tabs">
        {(
          [
            ["files", "文件"],
            ["music", "音乐"],
          ] as const
        ).map(([key, label]) => (
          <button
            key={key}
            className={"ws-tab" + (tab === key ? " active" : "")}
            onClick={() => setTab(key)}
          >
            {label}
          </button>
        ))}
      </div>
      {tab === "files" ? <WorkspaceFiles /> : <MusicPlayer />}
    </div>
  );
}
