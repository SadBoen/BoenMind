import { useEffect, useRef, useState } from "react";
import SessionList from "./SessionList";
import ChatPanel from "./ChatPanel";
import { rpc } from "../client";
import { setCurrentSession, useCurrentSession } from "../sessionStore";

// 聊天单元：同一 dockview 面板内的左右布局。
// 左 = Session 列表（可折叠；单元宽度不足时以悬浮窗形式打开；中间分割线可拖拽调宽）
// 右 = 聊天界面（消息流 + composer）
const SIDEBAR_W = 260;
const SIDEBAR_MIN = 180;
const SIDEBAR_MAX = 420;
const NARROW = 640;

export default function ChatUnit() {
  const containerRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(window.innerWidth);
  const [open, setOpen] = useState(true);
  const [floating, setFloating] = useState(false);
  const [sideW, setSideW] = useState(SIDEBAR_W);
  const currentId = useCurrentSession();

  // 挂载时确保有当前会话：有已存会话选第一个，否则新建。
  // （不然聊天输入框一直停在"请先选择或新建会话"禁用态，用户无法直接开聊。）
  useEffect(() => {
    if (currentId) return;
    (async () => {
      try {
        const v = await rpc<{ items: { sessionId: string }[] }>("session.list", {});
        if (v.items?.length) {
          setCurrentSession(v.items[0].sessionId);
          return;
        }
      } catch { /* 下面兜底新建 */ }
      try {
        const c = await rpc<{ sessionId: string }>("session.create", {});
        setCurrentSession(c.sessionId);
      } catch { /* 未登录等场景：留给用户手动操作 */ }
    })();
  }, [currentId]);

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

  // 分割线拖拽调宽：mousedown 记录起点，mousemove 算差值，mouseup 收尾。
  const dragRef = useRef<{ startX: number; startW: number } | null>(null);
  const onResizeStart = (e: React.MouseEvent) => {
    e.preventDefault();
    dragRef.current = { startX: e.clientX, startW: sideW };
    const onMove = (ev: MouseEvent) => {
      const d = dragRef.current;
      if (!d) return;
      const w = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, d.startW + ev.clientX - d.startX));
      setSideW(w);
    };
    const onUp = () => {
      dragRef.current = null;
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  };

  return (
    <div className="chat-unit" ref={containerRef}>
      {/* 会话列表收起/展开按钮：统一方形，竖向左缘中间（宽/窄模式一致）。
          展开时 ◀（朝左，提示可收起）+ 完整方框（四角圆角+四边框）；
          收起时 ▶（朝右，提示可展开）+ 方框转 180°（圆角朝右、开口朝左）。
          位置跟随分割线（sideW 拖拽后按钮跟着跑，不再硬编码 260px）。 */}
      <button
        className={`chat-unit-toggle ${(narrow ? floating : open) ? "active" : ""} ${narrow ? "narrow" : open ? "open" : "closed"}`}
        style={!narrow && open ? { left: `calc(${sideW}px - 13px)` } : undefined}
        title={narrow ? "会话列表" : open ? "收起会话" : "展开会话"}
        onClick={toggle}
      >
        {narrow ? "🗂️" : open ? "◀" : "▶"}
      </button>

      {/* 侧栏：宽模式内联（open 时显示），窄模式悬浮 overlay（floating 时显示） */}
      {!narrow && open && (
        <aside className="chat-unit-side" style={{ width: sideW }}>
          <SessionList onToggle={toggle} />
        </aside>
      )}
      {narrow && floating && (
        <aside className="chat-unit-side chat-unit-side-floating" style={{ width: SIDEBAR_W }}>
          <SessionList onToggle={toggle} floating />
        </aside>
      )}

      {/* 宽模式分割线：会话列表与聊天之间可拖拽调宽（窄模式悬浮窗无分割线） */}
      {!narrow && open && (
        <div className="chat-unit-resizer" onMouseDown={onResizeStart} title="拖拽调整会话列表宽度" />
      )}

      <div className="chat-unit-main">
        <ChatPanel />
      </div>
    </div>
  );
}
