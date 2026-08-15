/**
 * 启动画面：Cosmos 渐变 + logo + 版本 + 进度条（~2s），点击任意处跳过。
 * 展示后端版本号（health 拉到即显示；未就绪留空）。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles } from "lucide-react";
import { useAppStore } from "@/stores/app-store";

const BOOT_MS = 2000;

export function BootScreen({ onFinish }: { onFinish: () => void }) {
  const { t } = useTranslation();
  const version = useAppStore((s) => s.health?.version);
  const [progress, setProgress] = useState(0);

  // 进度条 ~2s 走完，随后自动进入桌面（跳过点击由 onPointerDown 拦截）
  useEffect(() => {
    const start = performance.now();
    const timer = setInterval(() => {
      const p = Math.min(1, (performance.now() - start) / BOOT_MS);
      setProgress(p);
      if (p >= 1) {
        clearInterval(timer);
        onFinish();
      }
    }, 16);
    return () => clearInterval(timer);
  }, [onFinish]);

  return (
    <div
      className="flex h-screen w-screen cursor-pointer select-none flex-col items-center justify-center gap-6 bg-gradient-to-br from-indigo-600 via-purple-700 to-sky-600"
      onPointerDown={onFinish}
      role="button"
      aria-label={t("desktop.bootSkip")}
    >
      {/* logo：渐变圆块 + 星光 */}
      <div className="flex h-20 w-20 items-center justify-center rounded-3xl bg-white/15 shadow-2xl backdrop-blur-xl">
        <Sparkles size={36} className="text-white" />
      </div>

      <div className="flex flex-col items-center gap-1.5">
        <h1 className="text-3xl font-bold tracking-tight text-white">BoenMind</h1>
        {version && <p className="text-sm text-white/70">v{version}</p>}
      </div>

      {/* 进度条 */}
      <div className="h-1.5 w-56 overflow-hidden rounded-full bg-white/20">
        <div
          className="h-full rounded-full bg-white transition-[width] duration-100 ease-linear"
          style={{ width: `${progress * 100}%` }}
        />
      </div>
    </div>
  );
}
