/**
 * 桌面：紫蓝渐变壁纸（明暗两套）+ 窗口层 + 任务栏 + 开始菜单。
 * 空桌面开机：应用手动从开始菜单打开。
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/app-store";
import { AppWindow } from "./AppWindow";
import { Taskbar } from "./Taskbar";
import { StartMenu } from "./StartMenu";

export function Desktop() {
  const { t } = useTranslation();
  const openApps = useAppStore((s) => s.openApps);
  const closeApp = useAppStore((s) => s.closeApp);
  const [startOpen, setStartOpen] = useState(false);

  return (
    <div className="relative h-screen w-screen select-none overflow-hidden bg-gradient-to-br from-indigo-200 via-purple-100 to-sky-200 dark:from-[#151040] dark:via-[#1a1040] dark:to-[#0d1b3e]">
      {/* 空桌面提示：无窗口时居中显示 */}
      {openApps.length === 0 && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
          <p className="rounded-2xl bg-white/40 px-5 py-2 text-sm text-indigo-900/70 backdrop-blur-md dark:bg-white/5 dark:text-white/50">
            {t("desktop.emptyHint")}
          </p>
        </div>
      )}

      {/* 窗口层：bottom 让位任务栏，作为 react-rnd 的 bounds */}
      <div className="absolute inset-x-0 bottom-12 top-0 z-10">
        {openApps.map((id, index) => (
          <AppWindow key={id} id={id} zIndex={index} onClose={closeApp} />
        ))}
      </div>

      <Taskbar startOpen={startOpen} onToggleStart={() => setStartOpen((v) => !v)} />
      {startOpen && <StartMenu onClose={() => setStartOpen(false)} />}
    </div>
  );
}
