import React, { useEffect, useState } from "react";
import { useAuiState } from "@assistant-ui/react";

/**
 * UserTurn: 用户历史发言数据模型
 */
export interface UserTurnItem {
  index: number;
  text: string;
  element?: HTMLElement | null;
}

/**
 * DotScrollbar: 用户发言导航点阵（User Turns Dot Navigator）
 * 满足用户核心设计要求：
 * 1. 小圆点阵列固定在屏幕高度中间（top: 50%, transform: translateY(-50%)），绝对不会因滚动而跑偏乱动；
 * 2. 一个圆点精准对应一句用户的发言（User Message）；
 * 3. 鼠标悬停在某个小圆点上时，向左侧平滑悬浮展开对应的发言文本预览（Tooltip）；
 * 4. 点击小圆点平滑滚动定位到该条发言处；
 * 5. 当前视口所处的最新发言对应的小圆点高亮/微扩大，其余圆点处于低饱和度温润态。
 */
export function DotScrollbar({
  viewportRef,
}: {
  viewportRef: React.RefObject<HTMLDivElement | null>;
}) {
  const [userTurns, setUserTurns] = useState<UserTurnItem[]>([]);
  const [activeTurnIdx, setActiveTurnIdx] = useState<number>(0);
  const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

  // 监听 thread 内的消息变化
  const messages = useAuiState((s) => s.thread.messages);

  useEffect(() => {
    const el = viewportRef.current;
    if (!el) return;

    const scanUserMessages = () => {
      const userNodes = el.querySelectorAll<HTMLElement>(".msg.user");
      const turns: UserTurnItem[] = [];
      userNodes.forEach((node, i) => {
        const textContent =
          node.querySelector(".content span")?.textContent?.trim() ||
          node.innerText.replace(/^我\n?/, "").trim() ||
          `第 ${i + 1} 条发言`;
        turns.push({
          index: i,
          text: textContent,
          element: node,
        });
      });
      setUserTurns(turns);
    };

    // 立即执行并配合短延时，确保动态新增消息后小圆点数量实时精准响应
    scanUserMessages();
    const t = setTimeout(scanUserMessages, 150);

    // MutationObserver 监听子节点增删
    const mo = new MutationObserver(scanUserMessages);
    mo.observe(el, { childList: true, subtree: true });

    return () => {
      clearTimeout(t);
      mo.disconnect();
    };
  }, [messages, viewportRef]);

  // 监听滚动，计算当前视口所处的消息轮次
  useEffect(() => {
    const el = viewportRef.current;
    if (!el || userTurns.length === 0) return;

    const handleScroll = () => {
      const vpTop = el.scrollTop;
      const vpHeight = el.clientHeight;
      const centerLine = vpTop + vpHeight / 2;

      let closestIdx = 0;
      let minDistance = Infinity;

      userTurns.forEach((turn, idx) => {
        if (!turn.element) return;
        const msgTop = turn.element.offsetTop;
        const dist = Math.abs(msgTop - centerLine);
        if (dist < minDistance) {
          minDistance = dist;
          closestIdx = idx;
        }
      });

      setActiveTurnIdx(closestIdx);
    };

    el.addEventListener("scroll", handleScroll, { passive: true });
    handleScroll();
    return () => el.removeEventListener("scroll", handleScroll);
  }, [userTurns, viewportRef]);

  // 点击小圆点平滑定位到对应发言
  const scrollToTurn = (turn: UserTurnItem) => {
    if (!turn.element || !viewportRef.current) return;
    turn.element.scrollIntoView({ behavior: "smooth", block: "center" });
  };

  // 如果还没有用户发言，默认显示一个初始待命小圆点（居中不会动）
  const displayTurns: Array<Partial<UserTurnItem> & { index: number; isPlaceholder?: boolean }> =
    userTurns.length > 0
      ? userTurns
      : [{ index: 0, text: "当前无历史发言", isPlaceholder: true }];

  return (
    <div
      className="dot-scrollbar-fixed-nav"
      data-slot="dot-scrollbar-fixed-nav"
    >
      <div className="dot-scrollbar-cluster">
        {displayTurns.map((turn, i) => {
          const isActive = !turn.isPlaceholder && activeTurnIdx === i;
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
              key={turn.index}
              className="dot-nav-item"
              onMouseEnter={() => setHoveredIdx(i)}
              onMouseLeave={() => setHoveredIdx(null)}
              onClick={() => {
                if (!turn.isPlaceholder && turn.element) {
                  scrollToTurn(turn as UserTurnItem);
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
