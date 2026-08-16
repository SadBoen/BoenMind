/**
 * 皮肤背景层（皮肤系统，2026-08-16）：玻璃皮肤的视觉内容物。
 * - 有背景图：cover 铺满（本地 dataURL 或外链 URL）。
 * - 无背景图：按皮肤色调（--skin-hue）生成默认渐变，保证玻璃材质有内容可磨。
 * - 明暗自适应遮罩：保证前景文字对比度（玻璃底色之上再压一层）。
 * 渲染在 App 根容器内 z-0，内容层（ClassicShell）在 z-10，组件无感。
 */
import { useAppStore } from "@/stores/app-store";

export function SkinBackground() {
  const skin = useAppStore((s) => s.skin);
  const bg = useAppStore((s) => s.skinBackground);
  if (skin !== "glass") return null;

  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 z-0 overflow-hidden">
      {bg ? (
        <img src={bg.value} alt="" className="h-full w-full object-cover" />
      ) : (
        <div
          className="h-full w-full"
          style={{
            background:
              "linear-gradient(135deg, hsl(var(--skin-hue, 250) 55% 88%), hsl(calc(var(--skin-hue, 250) + 70) 50% 78%))",
          }}
        />
      )}
      {/* 明暗遮罩：背景图之上、内容之下，压出可读对比度 */}
      <div className="absolute inset-0 bg-white/15 dark:bg-black/25" />
    </div>
  );
}
