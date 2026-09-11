import React, { useEffect, useMemo, useState } from "react";
import { useAuiState } from "@assistant-ui/react";

/**
 * DotScrollbar: 用户发言导航点阵（User Turns Dot Navigator）
 * 满足用户核心设计要求：
 * 1. 小圆点阵列固定在屏幕高度中间（top: 50%, transform: translateY(-50%)），绝对不会因滚动而跑偏乱动；
 * 2. 一个圆点精准对应一句用户的发言（User Message）；
 * 3. 鼠标悬停在某个小圆点上时，向左侧平滑悬浮展开对应的发言文本预览（Tooltip）；
 * 4. 点击小圆点平滑滚动定位到该条发言处；
 * 5. 当前视口所处的最新发言对应的小圆点高亮/微扩大，其余圆点处于低饱和度温润态。
 *
 * 数据驱动（2026-09-12 校准）:圆点列表自 thread.messages 派生,不再扒取
 * .msg.user 渲染产物(MutationObserver + 延时兜底整段删除);DOM 只承担
 * 滚动定位/高亮测量,经 UserMessage 挂的 data-msg-id 按消息 id 对应。
 * 注:窗口化渲染(#27)外更早消息无 DOM 元素,其圆点仅作列表占位不可点。
 */
interface UserTurnItem {
  id: string;
  text: string;
}

function extractText(content: unknown): string {
  if (typeof content === "string") return content.trim();
  if (!Array.isArray(content)) return "";
  return content
    .filter((p): p is { type: "text"; text: string } => p?.type === "text")
    .map((p) => p.text)
    .join("\n")
    .trim();
}

export function DotScrollbar({
  viewportRef,
}: {
  viewportRef: React.RefObject<HTMLDivElement | null>;
}) {
  const [activeTurnId, setActiveTurnId] = useState<string | null>(null);
  const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

  // 圆点列表 = 用户发言数据(单一事实源);文本经消息数据直取
  const messages = useAuiState((s) => s.thread.messages);
  const userTurns = useMemo(
    () =>
      messages
        .filter((m) => m.role === "user")
        .map((m) => ({ id: m.id, text: extractText(m.content) })),
    [messages],
  );

  // 监听滚动,计算当前视口所处发言轮次(元素测量经 data-msg-id 定位)
  useEffect(() => {
    const el = viewportRef.current;
    if (!el || userTurns.length === 0) return;

    const handleScroll = () => {
      const vpTop = el.scrollTop;
      const vpHeight = el.clientHeight;
      const centerLine = vpTop + vpHeight / 2;

      const nodes = el.querySelectorAll<HTMLElement>("[data-msg-id]");
      const byId = new Map<string, HTMLElement>();
      nodes.forEach((n) => byId.set(n.dataset.msgId ?? "", n));

      let closestId: string | null = null;
      let minDistance = Infinity;
      for (const turn of userTurns) {
        const node = byId.get(turn.id);
        if (!node) continue;
        const dist = Math.abs(node.offsetTop - centerLine);
        if (dist < minDistance) {
          minDistance = dist;
          closestId = turn.id;
        }
      }
      if (closestId) setActiveTurnId(closestId);
    };

    el.addEventListener("scroll", handleScroll, { passive: true });
    handleScroll();
    return () => el.removeEventListener("scroll", handleScroll);
  }, [userTurns, viewportRef]);

  // 点击小圆点平滑定位到对应发言
  const scrollToTurn = (turn: UserTurnItem) => {
    const el = viewportRef.current;
    if (!el) return;
    const node = el.querySelector<HTMLElement>(
      `[data-msg-id="${CSS.escape(turn.id)}"]`,
    );
    node?.scrollIntoView({ behavior: "smooth", block: "center" });
  };

  // 如果还没有用户发言，默认显示一个初始待命小圆点（居中不会动）
  const displayTurns: Array<UserTurnItem & { isPlaceholder?: boolean }> =
    userTurns.length > 0
      ? userTurns
      : [{ id: "", text: "当前无历史发言", isPlaceholder: true }];

  return (
    <div
      className="dot-scrollbar-fixed-nav"
      data-slot="dot-scrollbar-fixed-nav"
    >
      <div className="dot-scrollbar-cluster">
        {displayTurns.map((turn, i) => {
          const isActive = !turn.isPlaceholder && activeTurnId === turn.id;
          const isHovered = hoveredIdx === i;

          // 计算波浪鱼眼放大比例 (Mac Dock 鱼眼效果):
          // 悬停点 (dist=0) -> scale 1.75
          // 相邻第 1 层 (dist=1) -> scale 1.35
          // 相邻第 2 层 (dist=2) -> scale 1.15
          // 其余更远处 -> scale 1.0 (基准)
          let scale = 1.0;
          let extraOpacity = 0.45;

          if (hoveredIdx !== null) {
            const dist = Math.abs(hoveredIdx - i);
            if (dist === 0) {
              scale = 1.75;
              extraOpacity = 1.0;
            } else if (dist === 1) {
              scale = 1.35;
              extraOpacity = 0.8;
            } else if (dist === 2) {
              scale = 1.15;
              extraOpacity = 0.6;
            }
          } else if (isActive) {
            scale = 1.25;
            extraOpacity = 0.85;
          }

          return (
            <div
              key={turn.isPlaceholder ? "placeholder" : turn.id}
              className="dot-nav-item"
              onMouseEnter={() => setHoveredIdx(i)}
              onMouseLeave={() => setHoveredIdx(null)}
              onClick={() => {
                if (!turn.isPlaceholder) {
                  scrollToTurn(turn);
                }
              }}
            >
              {/* 悬浮发言预览气泡 (位于圆点左侧) */}
              {isHovered && turn.text && (
                <div className="dot-nav-tooltip animate-in fade-in zoom-in-95 duration-150">
                  <div className="dot-nav-tooltip-arrow" />
                  <div className="dot-nav-tooltip-header">
                    <span>发言 #{i + 1}</span>
                  </div>
                  <div className="dot-nav-tooltip-body">
                    {turn.text.length > 80 ? turn.text.slice(0, 80) + "…" : turn.text}
                  </div>
                </div>
              )}

              {/* 实体正圆点 */}
              <button
                type="button"
                className={`dot-nav-bullet ${isActive ? "active" : ""} ${
                  isHovered ? "hovered" : ""
                }`}
                style={{
                  transform: `scale(${scale})`,
                  opacity: extraOpacity,
                }}
                title={turn.text ? `发言: ${turn.text.slice(0, 30)}` : undefined}
                aria-label={`跳转到发言 ${i + 1}`}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}
