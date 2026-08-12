/**
 * 导航与设置页注册表：单一数据源。
 *
 * 新增导航（NavKey）或设置页（SettingsTab）时，只需在这里加一行 ——
 * `Record<NavKey, ...>` / `Record<SettingsTab, ...>` 保证漏改会在编译期报错，
 * 导航栏 / 二级面板 / 主面板 / 设置菜单自动联动。
 */
import type { ComponentType, ReactNode } from "react";
import {
  BookOpenText,
  FolderOpen,
  Images,
  Info,
  Lightbulb,
  MessageSquare,
  MonitorCog,
  Palette,
  Puzzle,
  Settings,
  Users,
  Wand2,
} from "lucide-react";
import { ChatWindow } from "@/components/chat/ChatWindow";
import { SessionList } from "@/components/chat/SessionList";
import { ExpertTeamDocs } from "@/components/team/ExpertTeamDocs";
import { SettingsMenu } from "@/components/settings/SettingsMenu";
import { AboutSettings } from "@/components/settings/AboutSettings";
import { AppearanceSettings } from "@/components/settings/AppearanceSettings";
import { PluginsSettings } from "@/components/settings/PluginsSettings";
import { ProviderSettings } from "@/components/settings/ProviderSettings";
import { RefinementSettings } from "@/components/settings/RefinementSettings";
import { SkillsSettings } from "@/components/settings/SkillsSettings";
import { WorkspaceSettings } from "@/components/settings/WorkspaceSettings";
import { useAppStore } from "@/stores/app-store";

export type NavKey = "chat" | "team" | "gallery" | "knowledge" | "settings";
export type SettingsTab =
  | "appearance"
  | "providers"
  | "workspace"
  | "plugins"
  | "skills"
  | "refinement"
  | "about";

export interface NavEntry {
  labelKey: string;
  icon: ReactNode;
  /** 放在导航栏底部独立区（设置） */
  bottom?: boolean;
  /** 未实现的功能占位（图库/知识库） */
  placeholder?: boolean;
  /** 二级面板内容（占位导航无） */
  secondary?: ComponentType;
  /** 主面板内容（占位导航无） */
  main?: ComponentType;
}

export interface SettingsEntry {
  labelKey: string;
  descKey: string;
  icon: ReactNode;
  component: ComponentType;
}

/** 主导航（表内顺序即导航栏顺序；新增 NavKey 必须在此登记，否则编译报错） */
export const NAV: Record<NavKey, NavEntry> = {
  chat: {
    labelKey: "nav.chat",
    icon: <MessageSquare size={20} />,
    secondary: SessionList,
    main: ChatWindow,
  },
  // 专家团队（阶段 0：功能描述文档；团队编排/协作实现见 docs/expert-team.md）
  team: {
    labelKey: "nav.team",
    icon: <Users size={20} />,
    main: ExpertTeamDocs,
  },
  gallery: { labelKey: "nav.gallery", icon: <Images size={20} />, placeholder: true },
  knowledge: { labelKey: "nav.knowledge", icon: <BookOpenText size={20} />, placeholder: true },
  settings: {
    labelKey: "nav.settings",
    icon: <Settings size={20} />,
    bottom: true,
    secondary: SettingsMenu,
    main: SettingsPage,
  },
};

/** 设置页注册表（表内顺序即设置菜单顺序；新增 SettingsTab 必须在此登记） */
export const SETTINGS: Record<SettingsTab, SettingsEntry> = {
  appearance: {
    labelKey: "settings.menu.appearance",
    descKey: "settings.menu.appearanceDesc",
    icon: <Palette size={16} />,
    component: AppearanceSettings,
  },
  providers: {
    labelKey: "settings.menu.providers",
    descKey: "settings.menu.providersDesc",
    icon: <MonitorCog size={16} />,
    component: ProviderSettings,
  },
  workspace: {
    labelKey: "settings.menu.workspace",
    descKey: "settings.menu.workspaceDesc",
    icon: <FolderOpen size={16} />,
    component: WorkspaceSettings,
  },
  plugins: {
    labelKey: "settings.menu.plugins",
    descKey: "settings.menu.pluginsDesc",
    icon: <Puzzle size={16} />,
    component: PluginsSettings,
  },
  skills: {
    labelKey: "settings.menu.skills",
    descKey: "settings.menu.skillsDesc",
    icon: <Wand2 size={16} />,
    component: SkillsSettings,
  },
  refinement: {
    labelKey: "settings.menu.refinement",
    descKey: "settings.menu.refinementDesc",
    icon: <Lightbulb size={16} />,
    component: RefinementSettings,
  },
  about: {
    labelKey: "settings.menu.about",
    descKey: "settings.menu.aboutDesc",
    icon: <Info size={16} />,
    component: AboutSettings,
  },
};

/** 设置主面板：按当前 tab 渲染对应设置页（带滚动容器） */
export function SettingsPage() {
  const settingsTab = useAppStore((s) => s.settingsTab);
  const { component: Page } = SETTINGS[settingsTab];
  return (
    <div className="h-full min-w-0 overflow-y-auto bg-background">
      <div className="mx-auto max-w-3xl px-6 py-6">
        <Page />
      </div>
    </div>
  );
}
