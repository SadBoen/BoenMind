/**
 * 应用注册器（DE 契约 §四·B）：桌面壳的单一数据源。
 *
 * 新增应用（AppId）时只需在这里登记一行 —— `Record<AppId, AppEntry>` 保证
 * 漏改会在编译期报错，开始菜单 / 任务栏 / 窗口层自动联动。
 * 应用 = 前端包：本期静态注册（内容组件零改动嵌入），动态加载留后续轮。
 *
 * 设置页注册表（SETTINGS）随导航表退役迁入本文件，由设置应用内部使用。
 */
import type { ComponentType, ReactNode } from "react";
import {
  Activity,
  Code2,
  Info,
  Lightbulb,
  MessageSquare,
  MonitorCog,
  Palette,
  Puzzle,
  Settings,
  Wand2,
} from "lucide-react";
import { ChatWindow } from "@/components/chat/ChatWindow";
import { SessionList } from "@/components/chat/SessionList";
import { CodingApp } from "@/components/coding/CodingApp";
import { SettingsMenu } from "@/components/settings/SettingsMenu";
import { AboutSettings } from "@/components/settings/AboutSettings";
import { AppearanceSettings } from "@/components/settings/AppearanceSettings";
import { PluginsSettings } from "@/components/settings/PluginsSettings";
import { ProviderSettings } from "@/components/settings/ProviderSettings";
import { RefinementSettings } from "@/components/settings/RefinementSettings";
import { SkillsSettings } from "@/components/settings/SkillsSettings";
import { StewardSettings } from "@/components/settings/StewardSettings";
import { WorkspaceSettings } from "@/components/settings/WorkspaceSettings";
import { useAppStore } from "@/stores/app-store";
import { FolderOpen } from "lucide-react";

export type AppId = "chat" | "coding" | "settings" | "plugins" | "steward";

export interface AppEntry {
  id: AppId;
  /** i18n key：应用显示名 */
  nameKey: string;
  icon: ReactNode;
  /** Cosmos 渐变底色（应用卡 / 标题栏图标背景） */
  gradient: string;
  component: ComponentType;
  defaultSize: { width: number; height: number };
}

/**
 * 应用注册表（表内顺序即开始菜单顺序）。
 * 内容组件全部是既有成熟组件零改动嵌入；壳层（窗口/任务栏）只认本表。
 */
export const APPS: Record<AppId, AppEntry> = {
  chat: {
    id: "chat",
    nameKey: "desktop.app.chat",
    icon: <MessageSquare size={18} />,
    gradient: "linear-gradient(135deg, #8b7cf6, #4ea8f0)",
    component: ChatAppView,
    defaultSize: { width: 1040, height: 700 },
  },
  coding: {
    id: "coding",
    nameKey: "desktop.app.coding",
    icon: <Code2 size={18} />,
    gradient: "linear-gradient(135deg, #34d399, #4ea8f0)",
    // M2：独立壳（文件树/编辑器/分支图 + 活任务清单）
    component: CodingApp,
    defaultSize: { width: 1080, height: 700 },
  },
  settings: {
    id: "settings",
    nameKey: "desktop.app.settings",
    icon: <Settings size={18} />,
    gradient: "linear-gradient(135deg, #f59e0b, #f472b6)",
    component: SettingsAppView,
    defaultSize: { width: 880, height: 660 },
  },
  plugins: {
    id: "plugins",
    nameKey: "desktop.app.plugins",
    icon: <Puzzle size={18} />,
    gradient: "linear-gradient(135deg, #a78bfa, #f472b6)",
    component: PluginsAppView,
    defaultSize: { width: 820, height: 640 },
  },
  steward: {
    id: "steward",
    nameKey: "desktop.app.steward",
    icon: <Activity size={18} />,
    gradient: "linear-gradient(135deg, #f472b6, #fb923c)",
    component: StewardAppView,
    defaultSize: { width: 820, height: 640 },
  },
};

export const APP_LIST: AppEntry[] = Object.values(APPS);

/**
 * 聊天应用 = 会话列表 + 聊天窗口双栏（原三栏壳的 secondary|main 组合）。
 * 内部布局与滚动语义由两个子组件自带，壳层只提供容器高度。
 */
function ChatAppView() {
  return (
    <div className="flex h-full min-h-0">
      <div className="w-64 shrink-0 border-r bg-background/60">
        <SessionList />
      </div>
      <div className="min-w-0 flex-1">
        <ChatWindow />
      </div>
    </div>
  );
}

/**
 * 设置应用 = 设置菜单 + 设置页双栏（原三栏壳的 secondary|main 组合）。
 * SettingsPage 自带滚动容器，菜单栏固定。
 */
function SettingsAppView() {
  return (
    <div className="flex h-full min-h-0">
      <div className="w-52 shrink-0 overflow-y-auto border-r bg-background/60">
        <SettingsMenu />
      </div>
      <div className="min-w-0 flex-1">
        <SettingsPage />
      </div>
    </div>
  );
}

/**
 * 编程应用占位已退役（M2 起由 CodingApp 真实壳接管）；删除占位组件与
 * codingComingSoon 提示文案（ClassicShell disabled 语义一并解除）。
 */

/** 单页应用的滚动容器（PluginsSettings/StewardSettings 原依赖三栏壳面板的滚动，此处由壳补齐） */
function ScrollPage({ children }: { children: ReactNode }) {
  // p-6 与 SettingsPage 内容边距一致（窗口化后内容贴边框会显得间距不对）
  return (
    <div className="h-full min-w-0 overflow-y-auto bg-background p-6">
      <div className="mx-auto max-w-3xl">{children}</div>
    </div>
  );
}

function PluginsAppView() {
  return (
    <ScrollPage>
      <PluginsSettings />
    </ScrollPage>
  );
}

function StewardAppView() {
  return (
    <ScrollPage>
      <StewardSettings />
    </ScrollPage>
  );
}

export type SettingsTab =
  | "appearance"
  | "providers"
  | "steward"
  | "workspace"
  | "plugins"
  | "skills"
  | "refinement"
  | "about";

export interface SettingsEntry {
  labelKey: string;
  descKey: string;
  icon: ReactNode;
  component: ComponentType;
}

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
  steward: {
    labelKey: "settings.menu.steward",
    descKey: "settings.menu.stewardDesc",
    icon: <Activity size={16} />,
    component: StewardSettings,
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
