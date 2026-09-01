// W2:三栏布局 + 可拖分隔条(会话列表↕对话区、对话区↕工作区;宽度持久化
// localStorage,刷新保持)+ 设置中心入口(W2 整页式)。对话区 = W1 原样。
// W3:主题系统启动恢复(useThemeBoot)+ 玻璃主题花瓣层(Petals)。
import { BoenmindRuntimeProvider } from "./w1/runtime";
import { Thread } from "./w1/thread";
import { SettingsPage } from "./w2/SettingsPage";
import { WorkspaceFiles } from "./w2/WorkspaceFiles";
import { Petals } from "./w3/Petals";
import { useThemeBoot, loadThemeState, type ThemeDef } from "./w3/themes";
import { useCallback, useEffect, useRef, useState } from "react";

type ThemeId = ThemeDef["id"];
import {
  MessageCircle,
  Calendar,
  Layers,
  Globe,
  Folder,
  ClipboardList,
  FileText,
  Settings,
  Plus,
  RefreshCw,
} from "lucide-react";

// 三栏宽度持久化(W2 验收门 4:刷新后布局保持)
const LAYOUT_KEY = "bm_layout";
type Layout = { sessions: number; workspace: number };
const DEFAULT_LAYOUT: Layout = { sessions: 260, workspace: 320 };
const LIMITS = {
  sessions: { min: 180, max: 440 },
  workspace: { min: 240, max: 560 },
};

function loadLayout(): Layout {
  try {
    const raw = localStorage.getItem(LAYOUT_KEY);
    if (!raw) return DEFAULT_LAYOUT;
    const v = JSON.parse(raw) as Partial<Layout>;
    return {
      sessions: clamp(v.sessions ?? DEFAULT_LAYOUT.sessions, LIMITS.sessions),
      workspace: clamp(
        v.workspace ?? DEFAULT_LAYOUT.workspace,
        LIMITS.workspace,
      ),
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
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(next));
  }, []);

  return (
    <BoenmindRuntimeProvider>
      <div
        className="app"
        style={{
          // 子元素顺序:rail, sessions, splitter, thread, splitter, workspace
          gridTemplateColumns: `52px ${layout.sessions}px 5px minmax(0, 1fr) 5px ${layout.workspace}px`,
        }}
      >
        {theme === "glass" ? <Petals /> : null}
        <Rail
          settingsActive={settingsOpen}
          onSettings={() => setSettingsOpen((v) => !v)}
        />
        <SessionPanel />
        <HSplitter
          onDrag={(dx) =>
            saveLayout({
              ...layout,
              sessions: clamp(layout.sessions + dx, LIMITS.sessions),
            })
          }
        />
        <Thread />
        <HSplitter
          onDrag={(dx) =>
            saveLayout({
              ...layout,
              workspace: clamp(layout.workspace - dx, LIMITS.workspace),
            })
          }
        />
        <WorkspacePanel />
        {settingsOpen && (
          <SettingsPage onClose={() => setSettingsOpen(false)} />
        )}
      </div>
    </BoenmindRuntimeProvider>
  );
}

// 拖宽分隔条:纯前端(pointer 事件),拖动期间直接改列宽。
function HSplitter({ onDrag }: { onDrag: (dx: number) => void }) {
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
      title="拖动调整列宽"
    />
  );
}

function Rail({
  settingsActive,
  onSettings,
}: {
  settingsActive: boolean;
  onSettings: () => void;
}) {
  // 图标:W1 为界面骨架占位(lucide 线性图标);聊天页可用,其余随后续 W 项启用
  return (
    <div className="rail">
      <button className="rail-btn active" title="聊天">
        <MessageCircle size={18} />
      </button>
      <button className="rail-btn" title="日程(规划中)">
        <Calendar size={18} />
      </button>
      <button className="rail-btn" title="组件(规划中)">
        <Layers size={18} />
      </button>
      <button className="rail-btn" title="浏览(规划中)">
        <Globe size={18} />
      </button>
      <button className="rail-btn" title="文件(规划中)">
        <Folder size={18} />
      </button>
      <button className="rail-btn" title="清单(规划中)">
        <ClipboardList size={18} />
      </button>
      <button className="rail-btn" title="文档(规划中)">
        <FileText size={18} />
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

function SessionPanel() {
  return (
    <div className="sessions">
      <div className="sessions-head">
        <span className="title">聊天</span>
        <button className="icon-chip" title="新建对话">
          <Plus size={16} />
        </button>
      </div>
      {/* 会话搜索框随会话列表真数据(W 后续)一起回来;禁用占位框会误导
          「打不进字」,先移除 */}
      <div className="chips">
        <span className="chip active">全部</span>
        <span className="chip">未分配</span>
      </div>
      <div className="session-item">
        <span className="name">BoenMind 对话</span>
        <span className="meta">刚刚</span>
      </div>
      <div className="sessions-empty">会话列表真数据随 W 后续接入</div>
    </div>
  );
}

function WorkspacePanel() {
  const [tab, setTab] = useState<"files" | "artifacts" | "todos">("files");
  const emptyText: Record<"artifacts" | "todos", string> = {
    artifacts: "产物面随 W 后续接入。",
    todos: "此会话暂无活动任务列表。",
  };
  return (
    <div className="workspace">
      <div className="workspace-head">
        <span className="title">WORKSPACE</span>
        <span className="actions">
          <button className="icon-chip" title="新建(规划中)">
            <Plus size={15} />
          </button>
          <button
            className="icon-chip"
            title="同步(重载目录树)"
            onClick={() => window.dispatchEvent(new CustomEvent("bm-ws-refresh"))}
          >
            <RefreshCw size={14} />
          </button>
        </span>
      </div>
      <div className="ws-tabs">
        {(
          [
            ["files", "文件"],
            ["artifacts", "产物"],
            ["todos", "待办"],
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
      {tab === "files" ? (
        <WorkspaceFiles />
      ) : (
        <div className="ws-empty">{emptyText[tab]}</div>
      )}
    </div>
  );
}
