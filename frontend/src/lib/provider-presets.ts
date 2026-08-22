import type { ProviderKind } from "../types";

export type ProviderGroup = "recommended" | "more" | "local";

export interface ProviderPreset {
  base_url: string;
  models: string[];
  group: ProviderGroup;
  needsKey?: boolean;
}

export const PROVIDER_PRESETS: Record<ProviderKind, ProviderPreset> = {
  minimax: {
    base_url: "https://api.minimaxi.com/v1",
    models: ["MiniMax-M3", "MiniMax-M2.7", "MiniMax-M2.7-highspeed"],
    group: "recommended",
    needsKey: true,
  },
  deepseek: {
    base_url: "https://api.deepseek.com/v1",
    models: ["deepseek-v4-flash", "deepseek-v4-pro"],
    group: "recommended",
    needsKey: true,
  },
  custom: {
    base_url: "",
    models: [],
    group: "local",
    needsKey: true,
  },
};

export const KIND_VALUES = Object.keys(PROVIDER_PRESETS) as ProviderKind[];

export const KIND_LABELS: Record<ProviderKind, string> = {
  minimax: "MiniMax",
  deepseek: "DeepSeek",
  custom: "自定义",
};

export const GROUP_LABELS: Record<ProviderGroup, string> = {
  recommended: "热门推荐",
  more: "更多",
  local: "本地与兼容",
};

export const SHAPE_LABELS = {
  "openai-compatible": "OpenAI 兼容",
  anthropic: "Anthropic 方言",
  gemini: "Gemini 方言",
} as const;

export function applyApiPresets(api: Record<string, string | null> | null) {
  if (!api) return;
  for (const kind of KIND_VALUES) {
    const url = api[kind];
    if (url) PROVIDER_PRESETS[kind].base_url = url;
  }
}

export const KIND_GROUPS: { group: ProviderGroup; kinds: ProviderKind[] }[] = (
  ["recommended", "more", "local"] as ProviderGroup[]
).map((group) => ({
  group,
  kinds: KIND_VALUES.filter((k) => PROVIDER_PRESETS[k].group === group),
}));

export const KIND_PRESETS = PROVIDER_PRESETS;
