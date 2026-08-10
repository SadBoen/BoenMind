/**
 * 消息指示条（参照 ZCode 消息区，置于右侧）：右侧一条细竖轨 + 多根小横条，
 * 只跟踪用户（人类）的输入 —— 一句输入一根小横条，位置对应消息在内容中的位置、
 * 长度随内容量；滚动时跟随，最新可见的一根亮色、其余暗色；hover 横条变长
 * 并弹出该条输入的内容预览，点击平滑滚动到对应消息。
 */
import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { cn } from "@/lib/utils";

interface IndicatorItem {
  idx: number;
  top: number;
  height: number;
  active: boolean;
  preview: string;
  el: HTMLElement;
}

export function ScrollIndicators({
  containerRef,
}: {
  containerRef: RefObject<HTMLDivElement | null>;
}) {
  const [tick, setTick] = useState(0);
  const [hoverIdx, setHoverIdx] = useState<number | null>(null);
  const railRef = useRef<HTMLDivElement>(null);

  // 滚动 / 尺寸变化时重算指示条
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

  // 只统计用户消息：一条输入一根横条
  const items = useMemo<IndicatorItem[]>(() => {
    const el = containerRef.current;
    if (!el) return [];
    const nodes = Array.from(
      el.querySelectorAll<HTMLElement>("[data-mid][data-role='user']"),
    );
    if (nodes.length === 0) return [];
    const total = el.scrollHeight;
    if (total <= el.clientHeight) return [];
    const viewBottom = el.scrollTop + el.clientHeight;
    let activeIdx = 0;
    const list = nodes.map((node, i) => {
      const top = node.offsetTop;
      const height = node.offsetHeight;
      if (top <= viewBottom) activeIdx = i;
      return {
        idx: i,
        top: (top / total) * 100,
        height: Math.max((height / total) * 100, 1.1),
        active: false,
        preview: node.getAttribute("data-preview") ?? "",
        el: node,
      };
    });
    if (list.length > 0) list[activeIdx].active = true;
    return list;
  }, [containerRef, tick]);

  if (items.length === 0) return null;

  const hovered = hoverIdx !== null ? items[hoverIdx] : null;

  return (
    <div
      ref={railRef}
      className="group/rail absolute bottom-2 right-1 top-2 z-20 w-3 rounded-full"
      onMouseLeave={() => setHoverIdx(null)}
    >
      {/* 细竖轨：默认几乎隐形，hover 时微微亮起 */}
      <div className="absolute inset-y-0 left-1/2 w-px -translate-x-1/2 rounded-full bg-foreground/10 transition-colors group-hover/rail:bg-foreground/20" />

      {items.map((item) => (
        <button
          key={item.idx}
          type="button"
          aria-label="跳转到该条消息"
          onMouseEnter={() => setHoverIdx(item.idx)}
          onClick={() => {
            const el = containerRef.current;
            if (!el) return;
            el.scrollTo({ top: Math.max(0, item.el.offsetTop - 8), behavior: "smooth" });
          }}
          className="absolute left-1/2 -translate-x-1/2 -translate-y-1/2"
          style={{ top: `${item.top}%` }}
        >
          <span
            className={cn(
              "block h-1.5 rounded-full transition-all duration-150",
              // 默认短横条：最新可见亮色，其余暗色；hover 时加长加亮
              item.active ? "bg-primary" : "bg-foreground/20",
              hoverIdx === item.idx ? "w-6 bg-primary/90" : "w-2.5",
            )}
          />
        </button>
      ))}

      {/* 悬停预览：fixed 定位以脱离窄 rail（否则 shrink-to-fit 被压成窄条） */}
      {hovered &&
        (() => {
          const rect = railRef.current?.getBoundingClientRect();
          if (!rect) return null;
          const y = Math.max(8, rect.top + (hovered.top / 100) * rect.height - 20);
          return (
            <div
              className="pointer-events-none fixed z-30 max-w-64 rounded-lg border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-lg"
              style={{ top: y, right: window.innerWidth - rect.left + 14 }}
            >
              <p className="line-clamp-3 whitespace-pre-wrap break-words leading-relaxed opacity-90">
                {hovered.preview || "（无内容）"}
              </p>
            </div>
          );
        })()}
    </div>
  );
}
