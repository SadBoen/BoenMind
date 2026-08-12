/**
 * 模型提供商品牌图标渲染。
 *
 * 图标组件与 hasColor 元数据在 lib/provider-presets.tsx 的单一数据源里声明；
 * 本文件只负责渲染（Color 图标自带品牌色直接渲染；Mono 图标用 currentColor
 * 继承主题文字色；无图标映射的 kind 回退为首字母方块）。
 */
import { PROVIDER_PRESETS, type IconComponent } from "@/lib/provider-presets";
import type { ProviderKind } from "@/api/client";

/** 从预设表派生图标映射（新增 kind 无需改动本文件） */
const PROVIDER_ICONS = Object.fromEntries(
  Object.entries(PROVIDER_PRESETS)
    .filter(([, p]) => p.icon)
    .map(([kind, p]) => [kind, { Icon: p.icon, hasColor: p.hasColor }]),
) as Partial<Record<ProviderKind, { Icon: IconComponent; hasColor: boolean }>>;

/** 无品牌图标时的回退：首字母方块（参考 pi-web 设计） */
function FallbackIcon({ kind, size }: { kind: string; size: number }) {
  const label =
    kind
      .split(/[-_]/)
      .filter(Boolean)
      .slice(0, 2)
      .map((part) => part[0])
      .join("")
      .toUpperCase() || "?";
  return (
    <span
      aria-hidden="true"
      className="inline-flex shrink-0 items-center justify-center rounded-lg border text-muted-foreground/70"
      style={{
        width: size,
        height: size,
        fontSize: Math.max(8, Math.floor(size * 0.42)),
        fontWeight: 600,
        lineHeight: 1,
      }}
    >
      {label}
    </span>
  );
}

/**
 * 提供商品牌图标。
 *
 * @param kind ProviderKind；`label` 提供时用于回退首字母（如 custom 显示 "+"）
 */
export function ProviderIcon({
  kind,
  size,
  label,
}: {
  kind: ProviderKind | string;
  size: number;
  label?: string;
}) {
  const pi = PROVIDER_ICONS[kind as ProviderKind];
  if (!pi) return <FallbackIcon kind={label ?? kind} size={size} />;
  // Color 图标自带品牌色；Mono 图标跟随主题文字色
  if (pi.hasColor) return <pi.Icon size={size} />;
  return <pi.Icon size={size} style={{ color: "var(--muted-foreground)" }} />;
}
