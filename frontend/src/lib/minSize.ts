import { useLayoutEffect, useRef, useState } from "react";

/** Convert a px min-width into RRP's 0–100 percentage minSize. Never pass px to minSize. */
export function pxToPercent(px: number, containerPx: number): number {
  if (containerPx <= 0) return 12;
  return Math.min(40, Math.max(8, (px / containerPx) * 100));
}

export function useMinSizePercent(minWidthPx: number, fallback = 12): {
  hostRef: React.RefObject<HTMLDivElement | null>;
  minSize: number;
} {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [minSize, setMinSize] = useState(fallback);

  useLayoutEffect(() => {
    const el = hostRef.current;
    if (!el) return;
    const apply = () => {
      const w = el.getBoundingClientRect().width;
      setMinSize(pxToPercent(minWidthPx, w));
    };
    apply();
    const ro = new ResizeObserver(apply);
    ro.observe(el);
    return () => ro.disconnect();
  }, [minWidthPx]);

  return { hostRef, minSize };
}
