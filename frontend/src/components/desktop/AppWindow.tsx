/**
 * 应用窗口：居中可拖拽（react-rnd）、单例聚焦、标题栏左红黄绿三圆点。
 * 红=关闭；黄=最小化（收进 Dock，点击 Dock 图标恢复）；绿=最大化/还原；
 * 内容区只做容器（明确高度语义），滚动由应用内容组件自行负责。
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Rnd } from "react-rnd";
import { useTranslation } from "react-i18next";
import { APPS, type AppId } from "@/lib/app-registry";
import { useAppStore } from "@/stores/app-store";
import { cn } from "@/lib/utils";

export function AppWindow({
  id,
  zIndex,
  onClose,
}: {
  id: AppId;
  zIndex: number;
  onClose: (id: AppId) => void;
}) {
  const { t } = useTranslation();
  const focusApp = useAppStore((s) => s.focusApp);
  const minimizeApp = useAppStore((s) => s.minimizeApp);
  const entry = APPS[id];
  const containerRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const [size, setSize] = useState(entry.defaultSize);
  const [maximized, setMaximized] = useState(false);
  // 最大化前的窗口状态（绿点还原用）
  const prevState = useRef<{ pos: { x: number; y: number }; size: { width: number; height: number } } | null>(null);
  // 层叠偏移基准：记住挂载时的 z 序（聚焦置顶会改 zIndex，但窗口位置不该跳回）
  const cascade = useRef(zIndex).current;

  // 挂载时按容器尺寸居中并钳制尺寸（小窗口/矮屏幕下不溢出到任务栏）；
  // 后开的窗口按打开顺序错开偏移，避免完全覆盖（点击露出的边缘即可聚焦置顶）
  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const w = Math.min(entry.defaultSize.width, Math.max(320, el.clientWidth - 16));
    const h = Math.min(entry.defaultSize.height, Math.max(320, el.clientHeight - 16));
    // 层叠偏移 48px：后开的窗口错开足够距离，下层窗口标题栏可见、点击即聚焦
    const offset = (cascade % 7) * 48;
    setSize({ width: w, height: h });
    setPos({
      x: Math.max(8, Math.round((el.clientWidth - w) / 2) + offset),
      y: Math.max(8, Math.round((el.clientHeight - h) / 2) + Math.round(offset * 0.6)),
    });
  }, [entry.defaultSize, cascade]);

  /** 绿点/双击标题栏：最大化 ↔ 还原（还原回最大化前的位置与尺寸） */
  const toggleMaximize = useCallback(() => {
    if (!pos) return;
    if (!maximized) {
      prevState.current = { pos, size };
      const el = containerRef.current;
      if (el) {
        setSize({ width: el.clientWidth - 16, height: el.clientHeight - 16 });
        setPos({ x: 8, y: 8 });
      }
      setMaximized(true);
    } else {
      if (prevState.current) {
        setPos(prevState.current.pos);
        setSize(prevState.current.size);
      }
      setMaximized(false);
    }
  }, [maximized, pos, size]);

  // 最大化窗口跟随容器尺寸变化（浏览器窗口 resize 时不溢出桌面区）
  useEffect(() => {
    const el = containerRef.current;
    if (!el || !maximized) return;
    const ro = new ResizeObserver(() => {
      setSize({ width: el.clientWidth - 16, height: el.clientHeight - 16 });
      setPos({ x: 8, y: 8 });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [maximized]);

  // 位置未就绪前不渲染，避免初始闪现左上角
  if (!pos) return <div ref={containerRef} className="pointer-events-none absolute inset-0" />;

  return (
    // 容器铺满窗口区仅用于测量居中；pointer-events-none 穿透，
    // 让点击落到窗口本体（含被遮挡窗口露出的边缘，点击即聚焦置顶）
    <div ref={containerRef} className="pointer-events-none absolute inset-0">
      <Rnd
        size={size}
        position={pos}
        onDragStop={(_e, d) => setPos({ x: d.x, y: d.y })}
        onResizeStop={(_e, _dir, ref) =>
          setSize({ width: ref.offsetWidth, height: ref.offsetHeight })
        }
        bounds="parent"
        enableResizing={!maximized}
        minWidth={320}
        minHeight={240}
        dragHandleClassName="app-window-titlebar"
        dragDisabled={maximized}
        className="rounded-2xl"
        style={{ zIndex, pointerEvents: "auto" }}
        // 点击窗口任意处（含内容区）即聚焦置顶
        onMouseDownCapture={() => focusApp(id)}
      >
        <section
          className={cn(
            "flex h-full flex-col overflow-hidden rounded-2xl border bg-card shadow-2xl backdrop-blur-xl",
            "border-black/10 dark:border-white/10",
          )}
        >
          {/* 标题栏：左红黄绿三圆点 + 渐变应用图标 + 名称；双击=最大化/还原 */}
          <header
            className={cn(
              "app-window-titlebar flex h-10 shrink-0 cursor-grab select-none items-center gap-3 border-b bg-muted/60 px-3 active:cursor-grabbing",
              maximized && "cursor-default active:cursor-default",
            )}
            onDoubleClick={toggleMaximize}
          >
            <div className="flex items-center gap-1.5" aria-hidden>
              <button
                type="button"
                aria-label={t("desktop.windowClose", { app: t(entry.nameKey) })}
                className="h-3 w-3 rounded-full bg-[#ff5f57] transition-colors hover:bg-[#ff5f57]/70"
                onClick={() => onClose(id)}
              />
              <button
                type="button"
                aria-label={t("desktop.windowMinimize", { app: t(entry.nameKey) })}
                className="h-3 w-3 rounded-full bg-[#febc2e] transition-colors hover:bg-[#febc2e]/70"
                onClick={() => minimizeApp(id)}
              />
              <button
                type="button"
                aria-label={t("desktop.windowMaximize", { app: t(entry.nameKey) })}
                className={cn(
                  "h-3 w-3 rounded-full bg-[#28c840] transition-colors hover:bg-[#28c840]/70",
                  // 最大化时显示两级亮度区分状态
                  maximized && "ring-1 ring-[#28c840]/60",
                )}
                onClick={toggleMaximize}
              />
            </div>
            <span
              className="flex h-6 w-6 items-center justify-center rounded-lg text-white shadow-sm"
              style={{ background: entry.gradient }}
            >
              {entry.icon}
            </span>
            <span className="text-sm font-medium text-foreground">{t(entry.nameKey)}</span>
          </header>
          {/* 内容区：明确高度语义，滚动归应用内容组件 */}
          <div className="min-h-0 flex-1 bg-background">
            <entry.component />
          </div>
          {/* 右下角 resize 角标（macOS 风格；最大化时隐藏） */}
          {!maximized && (
            <span
              aria-hidden
              className="pointer-events-none absolute bottom-1 right-1 h-3.5 w-3.5 opacity-40"
              style={{
                background:
                  "linear-gradient(135deg, transparent 50%, currentColor 50%)",
              }}
            />
          )}
        </section>
      </Rnd>
    </div>
  );
}
