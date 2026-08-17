/**
 * 聊天显示配置（2026-08-17，任务"不同 APP 不同输出内容配置"）：
 * 每个场景（APP）可独立配置工具调用显示档位，后续扩展显示项（结果预览、
 * 思考摘要等）在此加字段。
 *
 * 设计对齐 hermes 架构教训：wire 上只发原始数据，展示措辞/形态由客户端
 * 按场景自决 —— 配置表即客户端的"场景展示策略"。
 */
import type { ToolDisplayMode } from "./tool-summary";

export interface ChatDisplayProfile {
  /** 工具调用显示档位 */
  toolDisplay: ToolDisplayMode;
}

/** 场景（APP）→ 显示配置；缺省用 DEFAULT（chat） */
export const CHAT_DISPLAY_PROFILES: Record<string, ChatDisplayProfile> = {
  /** 聊天：默认摘要，参数只进展开区 */
  chat: { toolDisplay: "summary" },
  /** 编程：摘要 + 参数预览行（默认展开内容更足，方便盯执行过程） */
  coding: { toolDisplay: "full" },
  /** WIKI：以文档整理为主，摘要足够 */
  wiki: { toolDisplay: "summary" },
};

const DEFAULT_PROFILE: ChatDisplayProfile = { toolDisplay: "summary" };

/** 按场景解析显示档位（未知场景回退默认） */
export function toolDisplayForScene(scene: string): ToolDisplayMode {
  return (CHAT_DISPLAY_PROFILES[scene] ?? DEFAULT_PROFILE).toolDisplay;
}
