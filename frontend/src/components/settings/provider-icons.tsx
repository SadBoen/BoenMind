/**
 * 模型提供商品牌图标（迁移自 pi-web 的 provider-icons.tsx）。
 *
 * 来源 @lobehub/icons：Color 图标自带品牌色直接渲染；Mono 图标用 currentColor
 * 继承主题文字色（浅色/深色主题自适应）；无图标映射的 kind 回退为首字母方块。
 */
import type React from "react";
import AnthropicIcon from "@lobehub/icons/es/Anthropic/components/Mono";
import OpenAIIcon from "@lobehub/icons/es/OpenAI/components/Mono";
import GoogleColorIcon from "@lobehub/icons/es/Google/components/Color";
import DeepSeekColorIcon from "@lobehub/icons/es/DeepSeek/components/Color";
import MinimaxColorIcon from "@lobehub/icons/es/Minimax/components/Color";
import MoonshotIcon from "@lobehub/icons/es/Moonshot/components/Mono";
import ZhipuColorIcon from "@lobehub/icons/es/Zhipu/components/Color";
import QwenColorIcon from "@lobehub/icons/es/Qwen/components/Color";
import XAIIcon from "@lobehub/icons/es/XAI/components/Mono";
import OpenRouterColorIcon from "@lobehub/icons/es/OpenRouter/components/Color";
import MistralColorIcon from "@lobehub/icons/es/Mistral/components/Color";
import TogetherColorIcon from "@lobehub/icons/es/Together/components/Color";
import CerebrasColorIcon from "@lobehub/icons/es/Cerebras/components/Color";
import FireworksColorIcon from "@lobehub/icons/es/Fireworks/components/Color";
import HuggingFaceColorIcon from "@lobehub/icons/es/HuggingFace/components/Color";
import NvidiaColorIcon from "@lobehub/icons/es/Nvidia/components/Color";
import XiaomiMiMoIcon from "@lobehub/icons/es/XiaomiMiMo/components/Mono";
import ZAIIcon from "@lobehub/icons/es/ZAI/components/Mono";
import AntGroupColorIcon from "@lobehub/icons/es/AntGroup/components/Color";
import GroqIcon from "@lobehub/icons/es/Groq/components/Mono";
import OllamaIcon from "@lobehub/icons/es/Ollama/components/Mono";
import BasetenIcon from "@lobehub/icons/es/Baseten/components/Mono";
import type { ProviderKind } from "@/api/client";

export type IconComponent = React.ComponentType<{
  size?: number | string;
  style?: React.CSSProperties;
}>;

/** hasColor=true → Color 图标（自带品牌色，无外壳）；false → Mono 图标（currentColor 跟随主题） */
const PROVIDER_ICONS: Partial<Record<ProviderKind, { Icon: IconComponent; hasColor: boolean }>> = {
  openai: { Icon: OpenAIIcon, hasColor: false },
  anthropic: { Icon: AnthropicIcon, hasColor: false },
  gemini: { Icon: GoogleColorIcon, hasColor: true },
  minimax: { Icon: MinimaxColorIcon, hasColor: true },
  deepseek: { Icon: DeepSeekColorIcon, hasColor: true },
  openrouter: { Icon: OpenRouterColorIcon, hasColor: true },
  moonshot: { Icon: MoonshotIcon, hasColor: false },
  zhipu: { Icon: ZhipuColorIcon, hasColor: true },
  qwen: { Icon: QwenColorIcon, hasColor: true },
  xai: { Icon: XAIIcon, hasColor: false },
  zai: { Icon: ZAIIcon, hasColor: false },
  groq: { Icon: GroqIcon, hasColor: false },
  mistral: { Icon: MistralColorIcon, hasColor: true },
  together: { Icon: TogetherColorIcon, hasColor: true },
  cerebras: { Icon: CerebrasColorIcon, hasColor: true },
  fireworks: { Icon: FireworksColorIcon, hasColor: true },
  huggingface: { Icon: HuggingFaceColorIcon, hasColor: true },
  nvidia: { Icon: NvidiaColorIcon, hasColor: true },
  xiaomi: { Icon: XiaomiMiMoIcon, hasColor: false },
  antling: { Icon: AntGroupColorIcon, hasColor: true },
  baseten: { Icon: BasetenIcon, hasColor: false },
  ollama: { Icon: OllamaIcon, hasColor: false },
};

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
