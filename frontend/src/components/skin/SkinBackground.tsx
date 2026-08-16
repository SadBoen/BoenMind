/**
 * 皮肤背景层（皮肤系统，2026-08-16）：玻璃皮肤的视觉内容物。
 * 层序（z 递增）：壁纸（自定义图 > 预设壁纸 > 默认色调渐变）→ 背景特效层
 * （独立开关的动画层，见 effects.tsx）→ 明暗遮罩。
 * 优先级：自定义背景图 > 内置预设壁纸（渐变/流体两类）> 默认色调渐变。
 * - 自定义图：cover 铺满 + brightness/saturate 提亮（半透明白压层会吃掉色彩）。
 * - 渐变壁纸：PRESET_WALLPAPERS 声明的流体渐变（亮暗两套），零体积。
 * - 流体壁纸（蓝色波浪）：FluidWave WebGL 静态纹理，观感吸收 dsh-client-ui-aqua
 *   的 fluid shader（deepseek.com 官网同款风格），自研实现。
 * 渲染在 App 根容器内 z-0，内容层（ClassicShell）在 z-10，组件无感。
 */
import { useTheme } from "next-themes";
import { useAppStore } from "@/stores/app-store";
import { wallpaperById } from "@/lib/skin";
import { FluidWave } from "@/components/skin/FluidWave";
import { EffectWave } from "@/components/skin/effects";

export function SkinBackground() {
  const skin = useAppStore((s) => s.skin);
  const bg = useAppStore((s) => s.skinBackground);
  const wallpaperId = useAppStore((s) => s.skinWallpaper);
  const backgroundEffect = useAppStore((s) => s.backgroundEffect);
  const { resolvedTheme } = useTheme();
  if (skin !== "glass") return null;

  const preset = wallpaperById(wallpaperId);
  const isDark = resolvedTheme === "dark";
  const presetCss = preset && preset.kind === "gradient" ? (isDark ? preset.darkCss : preset.css) : null;
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
      ) : preset?.kind === "fluid" ? (
        <FluidWave />
      ) : (
        <div className="h-full w-full" style={{ background: presetCss ?? defaultCss }} />
      )}
      {/* 背景特效层（独立开关；wave = 蓝色波纹动画，自带流体底色盖过壁纸） */}
      {backgroundEffect === "wave" && <EffectWave />}
      {/* 明暗遮罩：背景图之上、内容之下，压出可读对比度（已极淡，避免压死背景） */}
      <div className="absolute inset-0 bg-white/10 dark:bg-black/20" />
    </div>
  );
}
