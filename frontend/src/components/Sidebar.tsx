import { useState } from "react";

export type AppView = "chat" | "coding";

interface Props {
  active: AppView;
  onSelect: (v: AppView) => void;
  onOpenSettings: () => void;
  onLogout: () => void;
}

export default function Sidebar({ active, onSelect, onOpenSettings, onLogout }: Props) {
  const [expanded, setExpanded] = useState<AppView | null>(null);

  const pick = (v: AppView) => {
    onSelect(v);
    setExpanded(expanded === v ? null : v);
  };

  return (
    <div className="sidebar">
      <div className="sidebar-icons">
        <button
          title="聊天"
          className={`sidebar-icon ${active === "chat" ? "active" : ""}`}
          onClick={() => pick("chat")}
        >
          💬
        </button>
        <button
          title="编程"
          className={`sidebar-icon ${active === "coding" ? "active" : ""}`}
          onClick={() => pick("coding")}
        >
          🧑‍💻
        </button>
        <button title="设置" className="sidebar-icon" onClick={onOpenSettings}>
          ⚙️
        </button>
        <div className="spacer" />
        <button className="sidebar-icon" title="退出登录" onClick={onLogout}>⏻</button>
      </div>
      {expanded && (
        <div className="sidebar-panel" onMouseLeave={() => setExpanded(null)}>
          <div className="sp-panel-title">{expanded === "chat" ? "聊天" : "编程"}</div>
        </div>
      )}
    </div>
  );
}
