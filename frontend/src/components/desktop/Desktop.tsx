/**
 * 桌面：顶部菜单栏 + 星空壁纸区（窗口层）+ 底部 Dock。
 * 布局为 flex 列：MenuBar（顶部）→ 桌面区（flex-1，壁纸+窗口）→ Dock（底部）。
 * 空桌面开机：应用手动从开始菜单 / Dock 打开。
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/app-store";
import { AppWindow } from "./AppWindow";
import { MenuBar } from "./MenuBar";
import { Dock } from "./Dock";
import { StatusBar } from "./StatusBar";
import { StartMenu } from "./StartMenu";

export function Desktop() {
  const { t } = useTranslation();
  const openApps = useAppStore((s) => s.openApps);
  const closeApp = useAppStore((s) => s.closeApp);
  const [startOpen, setStartOpen] = useState(false);

  return (
    // 壁纸在根容器：覆盖整屏（含 Dock/状态栏区域），避免底部露出 body 白底
    <div className="desktop-wallpaper relative flex h-screen w-screen select-none flex-col overflow-hidden">
      <MenuBar startOpen={startOpen} onToggleStart={() => setStartOpen((v) => !v)} />

      {/* 桌面区：窗口层（Dock 不占窗口层空间，bounds 自然限制在 Dock 之上） */}
      <div className="relative min-h-0 flex-1">
        {/* 空桌面提示 */}
        {openApps.length === 0 && (
          <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center">
            <p className="rounded-2xl bg-white/40 px-5 py-2 text-sm text-indigo-900/70 backdrop-blur-md dark:bg-black/30 dark:text-white/70">
              {t("desktop.emptyHint")}
            </p>
          </div>
        )}

        {/* 窗口层：react-rnd 的 bounds 容器 */}
        <div className="absolute inset-0 z-20">
          {openApps.map((id, index) => (
            <AppWindow key={id} id={id} zIndex={index} onClose={closeApp} />
          ))}
        </div>
      </div>

      <Dock />

      {/* 底部状态栏：版本/模型/工作目录/后端状态 */}
      <StatusBar />

      {startOpen && <StartMenu onClose={() => setStartOpen(false)} />}
    </div>
  );
}
