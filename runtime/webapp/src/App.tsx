// W1(ADR-0014):BoenMind WebUI 壳子——三栏布局骨架 + 对话闭环
// 布局蓝本与设计令牌见 milestones/W1-implementation-spec.md §3
import { BoenmindRuntimeProvider } from "./w1/runtime";
import { Thread } from "./w1/thread";
import { useState } from "react";
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

export default function App() {
  const [settingsOpen, setSettingsOpen] = useState(false);
  return (
    <BoenmindRuntimeProvider>
      <div className="app">
        <Rail onSettings={() => setSettingsOpen(true)} />
        <SessionPanel />
        <Thread />
        <WorkspacePanel />
        {settingsOpen && <SettingsDialog onClose={() => setSettingsOpen(false)} />}
      </div>
    </BoenmindRuntimeProvider>
  );
}

function Rail({ onSettings }: { onSettings: () => void }) {
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
      <button className="rail-btn" title="设置" onClick={onSettings}>
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
      <input className="search" placeholder="搜索对话…" disabled />
      <div className="chips">
        <span className="chip active">全部</span>
        <span className="chip">未分配</span>
      </div>
      <div className="session-item">
        <span className="name">BoenMind 对话</span>
        <span className="meta">刚刚</span>
      </div>
      <div className="sessions-empty">会话列表真数据随 W2 接入</div>
    </div>
  );
}

function WorkspacePanel() {
  const [tab, setTab] = useState<"files" | "artifacts" | "todos">("todos");
  const emptyText: Record<typeof tab, string> = {
    files: "文件面随 W2 接入。",
    artifacts: "产物面随 W2 接入。",
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
          <button className="icon-chip" title="同步(规划中)">
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
      <div className="ws-empty">{emptyText[tab]}</div>
    </div>
  );
}

function SettingsDialog({ onClose }: { onClose: () => void }) {
  return (
    <div
      className="overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="dialog">
        <div className="dialog-title">设置</div>
        <p className="dialog-text">
          设置页(模型/外观/偏好)随 W2 提供。当前模型接入已由服务器配置文件
          与环境变量驱动,无需手工填写。
        </p>
        <div className="dialog-actions">
          <button className="btn-primary" onClick={onClose}>
            知道了
          </button>
        </div>
      </div>
    </div>
  );
}
