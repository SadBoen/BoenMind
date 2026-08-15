/**
 * 模型提供商预设：展示数据（默认模型 / 选择器分组 / 品牌图标）。
 *
 * 新增提供商 kind 时：后端 bm_core::providers::official_base_url + ProviderKind::ALL
 * 加一行（端点唯一数据源），本表加一行（图标/分组/默认模型），i18n kinds 加翻译。
 * 端点经 `applyApiPresets` 由后端下发合并，不再在此维护。
 * `Record<ProviderKind, ...>` 保证漏加任何 kind 会在编译期报错
 * （ProviderKind union 也在 api/client.ts，两端一起改）。
 *
 * 厂商插件化方案 A（2026-08-16 拍板）：内置厂商精简为 minimax/deepseek/custom
 * 三家——其余厂商需要时经 custom（填端点 + 协议形状）或插件接入，不再进内置表。
 *
 * 预设数据（默认模型）迁移自 pi 注册表（@earendil-works/pi-ai 0.84），
 * 云端留空端点表示使用服务商官方默认。
 */
import type React from "react";
import DeepSeekColorIcon from "@lobehub/icons/es/DeepSeek/components/Color";
import MinimaxColorIcon from "@lobehub/icons/es/Minimax/components/Color";
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
  /** 品牌图标；null = 回退首字母方块（custom 等） */
  icon: IconComponent | null;
  /** true = Color 图标自带品牌色；false = Mono 图标 currentColor 跟随主题 */
  hasColor: boolean;
  /** 表单是否强制要求 API key；缺省 true（本地模型服务为 false） */
  needsKey?: boolean;
}

/** 表内声明顺序即表单 kind 下拉的展示顺序（按推荐分组排列） */
export const PROVIDER_PRESETS: Record<ProviderKind, ProviderPreset> = {
  // ── 推荐 ──
  minimax: { base_url: "https://api.minimaxi.com/v1", models: ["MiniMax-M3", "MiniMax-M2.7", "MiniMax-M2.7-highspeed"], group: "recommended", icon: MinimaxColorIcon, hasColor: true },
  deepseek: { base_url: "https://api.deepseek.com/v1", models: ["deepseek-v4-flash", "deepseek-v4-pro"], group: "recommended", icon: DeepSeekColorIcon, hasColor: true },
  // ── 本地与兼容 ──
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
