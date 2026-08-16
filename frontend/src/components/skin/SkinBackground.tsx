/**
 * 皮肤背景层（皮肤系统，2026-08-16）：玻璃皮肤的视觉内容物。
 * 优先级：自定义背景图 > 内置预设壁纸（明暗自适应渐变）> 默认色调渐变。
 * - 自定义图：cover 铺满 + brightness/saturate 提亮（半透明白压层会吃掉色彩）。
 * - 预设壁纸：lib/skin.ts PRESET_WALLPAPERS 声明的流体渐变（亮暗两套），
 *   观感吸收 dsh-client-ui-aqua 的流体壁纸，CSS 纯渐变零体积。
 * 渲染在 App 根容器内 z-0，内容层（ClassicShell）在 z-10，组件无感。
 */
import { useTheme } from "next-themes";
import { useAppStore } from "@/stores/app-store";
import { wallpaperById } from "@/lib/skin";

export function SkinBackground() {
  const skin = useAppStore((s) => s.skin);
  const bg = useAppStore((s) => s.skinBackground);
  const wallpaperId = useAppStore((s) => s.skinWallpaper);
  const { resolvedTheme } = useTheme();
  if (skin !== "glass") return null;

  const preset = wallpaperById(wallpaperId);
  const presetCss = preset ? (resolvedTheme === "dark" ? preset.darkCss : preset.css) : null;
  const defaultCss =
    "linear-gradient(135deg, hsl(var(--skin-hue, 250) 55% 88%), hsl(calc(var(--skin-hue, 250) + 70) 50% 78%))";

  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 z-0 overflow-hidden">
      {bg ? (
        <img
          src={bg.value}
          alt=""
          className="h-full w-full object-cover"
          style={{ filter: "brightness(1.12) saturate(1.18)" }}
        />
      ) : (
        <div className="h-full w-full" style={{ background: presetCss ?? defaultCss }} />
      )}
      {/* 明暗遮罩：背景图之上、内容之下，压出可读对比度（已极淡，避免压死背景） */}
      <div className="absolute inset-0 bg-white/10 dark:bg-black/20" />
    </div>
  );
}
