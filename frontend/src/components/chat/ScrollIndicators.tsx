/**
 * 消息指示条（参照 ZCode 消息区，置于右侧）：右侧一条细竖轨，
 * 竖轨竖直中间位置停着一簇紧凑的小横条 —— 一句用户（人类）输入一根，
 * 按时间从上到下排列，最新一根（簇底部）亮色、其余暗色；
 * hover 横条变长并弹出该条输入的内容预览，点击平滑滚动到对应消息。
 */
import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

export function ScrollIndicators({
  containerRef,
}: {
  containerRef: RefObject<HTMLDivElement | null>;
}) {
  const { t } = useTranslation();
  const [tick, setTick] = useState(0);
  const [hoverIdx, setHoverIdx] = useState<number | null>(null);
  const railRef = useRef<HTMLDivElement>(null);

  // 消息变化 / 尺寸变化时刷新（新输入到达、会话切换等）
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    let raf = 0;
    const update = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => setTick((t) => t + 1));
    };
    el.addEventListener("scroll", update, { passive: true });
    const ro = new ResizeObserver(update);
    ro.observe(el);
    update();
    return () => {
      el.removeEventListener("scroll", update);
      ro.disconnect();
      cancelAnimationFrame(raf);
    };
  }, [containerRef]);

  // 只统计用户消息：一句输入一根横条（按时间顺序，最新在最下面）
  const items = useMemo(() => {
    const el = containerRef.current;
    if (!el) return [];
    return Array.from(
      el.querySelectorAll<HTMLElement>("[data-mid][data-role='user']"),
    ).map((node, i) => ({
      idx: i,
      preview: node.getAttribute("data-preview") ?? "",
      el: node,
    }));
  }, [containerRef, tick]);

  if (items.length === 0) return null;

  const hovered = hoverIdx !== null ? items[hoverIdx] : null;
  const lastIdx = items.length - 1;

  return (
    <div
      ref={railRef}
      className="group/rail absolute bottom-2 right-1 top-2 z-20 w-3 rounded-full"
      onMouseLeave={() => setHoverIdx(null)}
    >
      {/* 横条簇：紧凑排列，整体固定在竖直中间（无竖线） */}
      <div className="absolute left-1/2 top-1/2 flex -translate-x-1/2 -translate-y-1/2 flex-col items-center gap-1.5">
        {items.map((item) => (
          <button
            key={item.idx}
            type="button"
            aria-label={t("chat.scroll.jumpToMessage")}
            onMouseEnter={() => setHoverIdx(item.idx)}
            onClick={() => {
              const el = containerRef.current;
              if (!el) return;
              el.scrollTo({ top: Math.max(0, item.el.offsetTop - 8), behavior: "smooth" });
            }}
          >
            <span
              className={cn(
                "block h-1 rounded-full transition-all duration-150",
                // 最新一根（簇底部）亮色，其余暗色；hover 时加长加亮
                item.idx === lastIdx ? "bg-primary" : "bg-foreground/25",
                hoverIdx === item.idx ? "w-6 bg-primary/90" : "w-2.5",
              )}
            />
          </button>
        ))}
      </div>

      {/* 悬停预览：fixed 定位以脱离窄 rail（否则 shrink-to-fit 被压成窄条） */}
      {hovered &&
        (() => {
          const rect = railRef.current?.getBoundingClientRect();
          if (!rect) return null;
          // 横条簇固定在轨道中间
          const y = Math.max(8, rect.top + rect.height / 2 - 20);
          return (
            <div
              className="pointer-events-none fixed z-30 max-w-64 rounded-lg border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-lg"
              style={{ top: y, right: window.innerWidth - rect.left + 14 }}
            >
              <p className="line-clamp-3 whitespace-pre-wrap break-words leading-relaxed opacity-90">
                {hovered.preview || t("chat.scroll.emptyPreview")}
              </p>
            </div>
          );
        })()}
    </div>
  );
}
