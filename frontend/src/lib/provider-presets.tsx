/**
 * 模型提供商预设：展示数据（默认模型 / 选择器分组 / 品牌图标）。
 *
 * 新增提供商 kind 时：后端 bm_core::providers::official_base_url + ProviderKind::ALL
 * 加一行（端点唯一数据源），本表加一行（图标/分组/默认模型），i18n kinds 加翻译。
 * 端点经 `applyApiPresets` 由后端下发合并，不再在此维护。
 * `Record<ProviderKind, ...>` 保证漏加任何 kind 会在编译期报错
 * （ProviderKind union 也在 api/client.ts，两端一起改）。
 *
 * 预设数据（默认模型）迁移自 pi 注册表（@earendil-works/pi-ai 0.84），
 * 云端留空端点表示使用服务商官方默认。
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

/** 选择器分组（key 用 settings.providers.picker.<group> 翻译） */
export type ProviderGroup = "recommended" | "more" | "local";

export interface ProviderPreset {
  /** 官方端点（空 = 使用服务商默认） */
  base_url: string;
  /** 预设模型（2026-08，pi-ai 0.84.1 模型目录） */
  models: string[];
  group: ProviderGroup;
  /** 品牌图标；null = 回退首字母方块（custom/llamacpp 等） */
  icon: IconComponent | null;
  /** true = Color 图标自带品牌色；false = Mono 图标 currentColor 跟随主题 */
  hasColor: boolean;
  /** 表单是否强制要求 API key；缺省 true（本地模型服务 ollama/llamacpp 为 false） */
  needsKey?: boolean;
}

/** 表内声明顺序即表单 kind 下拉的展示顺序（按推荐分组排列） */
export const PROVIDER_PRESETS: Record<ProviderKind, ProviderPreset> = {
  // ── 推荐 ──
  openai: { base_url: "", models: ["gpt-4o", "gpt-4o-mini", "gpt-4.1", "gpt-4.1-mini", "o3-mini"], group: "recommended", icon: OpenAIIcon, hasColor: false },
  anthropic: { base_url: "", models: ["claude-opus-4-5", "claude-sonnet-4-5", "claude-haiku-4-5"], group: "recommended", icon: AnthropicIcon, hasColor: false },
  gemini: { base_url: "", models: ["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.5-flash-lite", "gemini-3-flash-preview"], group: "recommended", icon: GoogleColorIcon, hasColor: true },
  deepseek: { base_url: "https://api.deepseek.com/v1", models: ["deepseek-v4-flash", "deepseek-v4-pro"], group: "recommended", icon: DeepSeekColorIcon, hasColor: true },
  minimax: { base_url: "https://api.minimaxi.com/v1", models: ["MiniMax-M3", "MiniMax-M2.7", "MiniMax-M2.7-highspeed"], group: "recommended", icon: MinimaxColorIcon, hasColor: true },
  moonshot: { base_url: "https://api.moonshot.cn/v1", models: ["kimi-k2.7-code", "kimi-k2.6", "kimi-k2-thinking", "kimi-k2.5"], group: "recommended", icon: MoonshotIcon, hasColor: false },
  zhipu: { base_url: "https://open.bigmodel.cn/api/paas/v4", models: ["glm-5.2", "glm-5.1", "glm-4.7", "glm-5.2-highspeed"], group: "recommended", icon: ZhipuColorIcon, hasColor: true },
  qwen: { base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1", models: ["qwen3.8-max", "qwen3.7-max", "qwen3.7-plus", "qwen3.6-flash"], group: "recommended", icon: QwenColorIcon, hasColor: true },
  // ── 更多 ──
  openrouter: { base_url: "https://openrouter.ai/api/v1", models: ["openai/gpt-4o", "anthropic/claude-sonnet-4-5", "deepseek/deepseek-v4-flash"], group: "more", icon: OpenRouterColorIcon, hasColor: true },
  xai: { base_url: "https://api.x.ai/v1", models: ["grok-4.5", "grok-4.3"], group: "more", icon: XAIIcon, hasColor: false },
  zai: { base_url: "https://api.z.ai/api/paas/v4", models: ["glm-5.2", "glm-5-turbo", "glm-4.7"], group: "more", icon: ZAIIcon, hasColor: false },
  groq: { base_url: "https://api.groq.com/openai/v1", models: ["llama-3.3-70b-versatile", "openai/gpt-oss-120b", "qwen/qwen3.6-27b"], group: "more", icon: GroqIcon, hasColor: false },
  mistral: { base_url: "https://api.mistral.ai/v1", models: ["mistral-large-latest", "mistral-medium-latest", "codestral-latest", "mistral-small-latest"], group: "more", icon: MistralColorIcon, hasColor: true },
  together: { base_url: "https://api.together.ai/v1", models: ["Qwen/Qwen3.7-Max", "deepseek-ai/DeepSeek-V4-Pro", "MiniMaxAI/MiniMax-M3"], group: "more", icon: TogetherColorIcon, hasColor: true },
  cerebras: { base_url: "https://api.cerebras.ai/v1", models: ["zai-glm-4.7", "openai/gpt-oss-120b", "gemma-4-31b"], group: "more", icon: CerebrasColorIcon, hasColor: true },
  fireworks: { base_url: "https://api.fireworks.ai/inference", models: ["accounts/fireworks/models/deepseek-v4-flash", "accounts/fireworks/models/kimi-k2p7-code", "accounts/fireworks/models/gpt-oss-120b"], group: "more", icon: FireworksColorIcon, hasColor: true },
  huggingface: { base_url: "https://router.huggingface.co/v1", models: ["Qwen/Qwen3-235B-A22B", "deepseek-ai/DeepSeek-V4-Pro", "MiniMaxAI/MiniMax-M3", "zai-org/GLM-5.2"], group: "more", icon: HuggingFaceColorIcon, hasColor: true },
  nvidia: { base_url: "https://integrate.api.nvidia.com/v1", models: ["meta/llama-3.3-70b-instruct", "meta/llama-3.1-8b-instruct", "minimaxai/minimax-m3"], group: "more", icon: NvidiaColorIcon, hasColor: true },
  xiaomi: { base_url: "https://api.xiaomimimo.com/v1", models: ["mimo-v2.5-pro", "mimo-v2.5", "mimo-v2-pro", "mimo-v2-flash"], group: "more", icon: XiaomiMiMoIcon, hasColor: false },
  antling: { base_url: "https://api.ant-ling.com/v1", models: ["Ling-2.6-1T", "Ling-2.6-flash", "Ring-2.6-1T"], group: "more", icon: AntGroupColorIcon, hasColor: true },
  baseten: { base_url: "https://inference.baseten.co/v1", models: ["deepseek-ai/DeepSeek-V4-Flash-0731", "moonshotai/Kimi-K2.7-Code"], group: "more", icon: BasetenIcon, hasColor: false },
  // ── 本地与兼容 ──
  ollama: { base_url: "http://127.0.0.1:11434/v1", models: ["qwen3:8b", "llama3.1:8b"], group: "local", icon: OllamaIcon, hasColor: false, needsKey: false },
  llamacpp: { base_url: "http://127.0.0.1:8080/v1", models: [], group: "local", icon: null, hasColor: false, needsKey: false },
  custom: { base_url: "", models: [], group: "local", icon: null, hasColor: false },
};

/** 表单 kind 下拉的展示顺序（表内声明顺序） */
export const KIND_VALUES = Object.keys(PROVIDER_PRESETS) as ProviderKind[];

/**
 * 把后端下发的官方端点表合并进预设（端点唯一数据源在后端）。
 * 拉取失败 / 旧后端（返回 null）时保持本地值兜底，不阻塞 UI。
 * 调用方在挂载后调用并在完成后触发重渲染。
 */
export function applyApiPresets(api: Record<string, string | null> | null) {
  if (!api) return;
  for (const kind of KIND_VALUES) {
    const url = api[kind];
    if (url) PROVIDER_PRESETS[kind].base_url = url;
  }
}

/** 选择器分组（键顺序即展示顺序，由表的 group 字段派生） */
export const KIND_GROUPS: { group: ProviderGroup; kinds: ProviderKind[] }[] = (
  ["recommended", "more", "local"] as ProviderGroup[]
).map((group) => ({
  group,
  kinds: KIND_VALUES.filter((k) => PROVIDER_PRESETS[k].group === group),
}));

/** 端点 + 默认模型预设（兼容旧引用名） */
export const KIND_PRESETS: Record<ProviderKind, { base_url: string; models: string[]; needsKey?: boolean }> =
  PROVIDER_PRESETS;
