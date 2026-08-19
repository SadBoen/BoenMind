import { useEffect, useRef, useState } from "react";
import SessionList from "./SessionList";
import ChatPanel from "./ChatPanel";

// 聊天单元：同一 dockview 面板内的左右布局。
// 左 = Session 列表（可折叠；单元宽度不足时以悬浮窗形式打开）
// 右 = 聊天界面（消息流 + composer）
const SIDEBAR_W = 260;
const NARROW = 640;

export default function ChatUnit() {
  const containerRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(window.innerWidth);
  const [open, setOpen] = useState(true);
  const [floating, setFloating] = useState(false);

  // 监听容器宽度：不足阈值 = 窄模式（侧栏转悬浮窗）
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      for (const e of entries) setWidth(e.contentRect.width);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const narrow = width < NARROW;

  const toggle = () => {
    if (narrow) setFloating((f) => !f);
    else setOpen((o) => !o);
  };

  return (
    <div className="chat-unit" ref={containerRef}>
      {/* 会话列表收起/展开按钮：统一方形，竖向左缘中间（宽/窄模式一致） */}
      <button
        className={`chat-unit-toggle ${(narrow ? floating : open) ? "active" : ""} ${narrow ? "narrow" : open ? "open" : "closed"}`}
        title={narrow ? "会话列表" : open ? "收起会话" : "展开会话"}
        onClick={toggle}
      >
        {narrow ? "🗂️" : open ? "◀" : "▶"}
      </button>

      {/* 侧栏：宽模式内联（open 时显示），窄模式悬浮 overlay（floating 时显示） */}
      {!narrow && open && (
        <aside className="chat-unit-side" style={{ width: SIDEBAR_W }}>
          <SessionList onToggle={toggle} />
        </aside>
      )}
      {narrow && floating && (
        <aside className="chat-unit-side chat-unit-side-floating" style={{ width: SIDEBAR_W }}>
          <SessionList onToggle={toggle} floating />
        </aside>
      )}

      <div className="chat-unit-main">
        <ChatPanel />
      </div>
    </div>
  );
}
