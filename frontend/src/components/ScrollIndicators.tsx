import { useEffect, useMemo, useRef, useState, type RefObject } from "react";

export function ScrollIndicators({ containerRef }: { containerRef: RefObject<HTMLDivElement | null> }) {
  const [tick, setTick] = useState(0);
  const [hoverIdx, setHoverIdx] = useState<number | null>(null);
  const railRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    let raf = 0;
    const schedule = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => setTick((n) => n + 1));
    };
    const mo = new MutationObserver(schedule);
    mo.observe(el, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-preview"] });
    const ro = new ResizeObserver(schedule);
    ro.observe(el);
    schedule();
    return () => {
      mo.disconnect();
      ro.disconnect();
      cancelAnimationFrame(raf);
    };
  }, [containerRef]);

  const items = useMemo(() => {
    const el = containerRef.current;
    if (!el) return [];
    return Array.from(el.querySelectorAll<HTMLElement>("[data-mid][data-role='user']")).map((node, i) => ({
      idx: i,
      preview: node.getAttribute("data-preview") ?? "",
      node,
    }));
  }, [containerRef, tick]);

  if (items.length === 0) return null;
  const hovered = hoverIdx !== null ? items[hoverIdx] : null;
  const lastIdx = items.length - 1;

  return (
    <div ref={railRef} className="scroll-rail" onMouseLeave={() => setHoverIdx(null)}>
      <div className="scroll-ticks">
        {items.map((item) => (
          <button
            key={item.idx}
            type="button"
            className={`scroll-tick${item.idx === lastIdx ? " is-last" : ""}${hoverIdx === item.idx ? " is-hot" : ""}`}
            aria-label="跳到这条消息"
            onMouseEnter={() => setHoverIdx(item.idx)}
            onClick={() => {
              const el = containerRef.current;
              if (!el) return;
              el.scrollTo({ top: Math.max(0, item.node.offsetTop - 8), behavior: "smooth" });
            }}
          />
        ))}
      </div>
      {hovered && railRef.current && (
        <div
          className="scroll-preview"
          style={{
            top: Math.max(8, railRef.current.getBoundingClientRect().top + railRef.current.getBoundingClientRect().height / 2 - 20),
            right: window.innerWidth - railRef.current.getBoundingClientRect().left + 14,
          }}
        >
          {hovered.preview || "（无预览）"}
        </div>
      )}
    </div>
  );
}
