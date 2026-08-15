/**
 * Dock（macOS 磁吸风格）：底部居中的毛玻璃胶囊——5 个应用图标（Cosmos 渐变方块）。
 *
 * 磁吸机制移植自 PuruVJ/macos-web（评估表认定：注册制/分层教科书级，2.6K⭐ MIT）：
 * 鼠标在 Dock 上横向移动时，图标按"到鼠标的横向距离"分段放大——最近的最大、
 * 相邻次之、再远恢复原尺寸；宽度参与布局（flex），中心放大时两侧图标被平滑推开，
 * 这就是 macOS Dock 的经典磁吸效果。
 *
 * 运行中的应用有白色指示点；点击：未开则打开，已开则聚焦置顶。
 */
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { APP_LIST } from "@/lib/app-registry";
import { useAppStore } from "@/stores/app-store";
import { cn } from "@/lib/utils";

const BASE = 40; // 图标基准尺寸 px
/** 磁吸影响半径（到鼠标距离超过即恢复原尺寸） */
const RADIUS = 88;
/** 最大放大系数（中心图标） */
const MAX_SCALE = 1.4;

export function Dock() {
  const { t } = useTranslation();
  const openApps = useAppStore((s) => s.openApps);
  const focusedApp = useAppStore((s) => s.focusedApp);
  const minimized = useAppStore((s) => s.minimized);
  const openApp = useAppStore((s) => s.openApp);
  const focusApp = useAppStore((s) => s.focusApp);
  const restoreApp = useAppStore((s) => s.restoreApp);
  const [mouseX, setMouseX] = useState<number | null>(null);
  const btnRefs = useRef<(HTMLButtonElement | null)[]>([]);

  /** 按图标中心到鼠标的水平距离计算放大系数（线性衰减，最近 MAX_SCALE，半径外 1） */
  const scaleFor = (index: number) => {
    if (mouseX === null) return 1;
    const btn = btnRefs.current[index];
    if (!btn) return 1;
    const rect = btn.getBoundingClientRect();
    const dist = Math.abs(rect.left + rect.width / 2 - mouseX);
    if (dist >= RADIUS) return 1;
    return 1 + (1 - dist / RADIUS) * (MAX_SCALE - 1);
  };

  return (
    <nav
      aria-label={t("desktop.dock")}
      className="z-30 flex h-16 shrink-0 items-end justify-center pb-1.5"
    >
      <div
        className="flex items-end gap-1.5 rounded-2xl border border-white/15 bg-black/25 px-2 pb-1.5 pt-2 shadow-lg backdrop-blur-2xl"
        onMouseMove={(e) => setMouseX(e.clientX)}
        onMouseLeave={() => setMouseX(null)}
      >
        {APP_LIST.map((app, index) => {
          const open = openApps.includes(app.id);
          const active = focusedApp === app.id;
          const isMinimized = open && minimized.includes(app.id);
          const scale = scaleFor(index);
          return (
            <button
              key={app.id}
              ref={(el) => {
                btnRefs.current[index] = el;
              }}
              type="button"
              aria-label={t(app.nameKey)}
              onClick={() => {
                if (!open) {
                  openApp(app.id);
                } else if (isMinimized) {
                  // 最小化的窗口：Dock 点击恢复
                  restoreApp(app.id);
                } else {
                  focusApp(app.id);
                }
              }}
              className="group flex flex-col items-center"
            >
              <span
                className={cn(
                  "flex items-center justify-center rounded-xl text-white shadow-md",
                  active && "ring-2 ring-white/50",
                  // 最小化窗口的图标变暗（macOS 语义：最小化窗口在 Dock 中呈"收起"态）
                  isMinimized && "opacity-50 saturate-50",
                )}
                style={{
                  width: BASE * scale,
                  height: BASE * scale,
                  background: app.gradient,
                  transition: "width 120ms ease-out, height 120ms ease-out",
                }}
              >
                {app.icon}
              </span>
              {/* 运行指示点：最小化的窗口显示空心（仍存活但未在桌面） */}
              <span
                className={cn(
                  "mt-1 h-1 w-1 rounded-full bg-white/80 transition-opacity",
                  open ? "opacity-100" : "opacity-0",
                  isMinimized && "h-1.5 w-1.5 border border-white/60 bg-transparent",
                )}
              />
            </button>
          );
        })}
      </div>
    </nav>
  );
}
