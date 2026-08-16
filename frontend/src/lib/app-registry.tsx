/**
 * 应用注册器（DE 契约 §四·B）：壳层（导航条/主面板）的单一数据源。
 *
 * 新增应用（AppId）时只需在这里登记一行 —— `Record<AppId, AppEntry>` 保证
 * 漏改会在编译期报错，导航条自动联动。plugins/steward 两条目已随桌面壳
 * 退役收口（2026-08-16 清理轮）：内容组件经 SETTINGS 设置页可达，不再
 * 占用 AppId。应用 = 前端包：本期静态注册（内容组件零改动嵌入），
 * 动态加载留后续轮（§四·C）。
 */
import type { ComponentType, ReactNode } from "react";
import {
  Activity,
  BookOpen,
  Code2,
  Info,
  Lightbulb,
  MessageSquare,
  Cable,
  MonitorCog,
  Palette,
  Puzzle,
  ScrollText,
  Settings,
  Users,
  Wand2,
} from "lucide-react";
import { CodingApp } from "@/components/coding/CodingApp";
import { WikiApp } from "@/components/wiki/WikiApp";
import { DockLayout } from "@/components/layout/DockLayout";
import { SettingsMenu } from "@/components/settings/SettingsMenu";
import { LogsSettings } from "@/components/settings/LogsSettings";
import { AboutSettings } from "@/components/settings/AboutSettings";
import { AppearanceSettings } from "@/components/settings/AppearanceSettings";
import { AppSettings } from "@/components/settings/AppSettings";
import { ExpertsSettings } from "@/components/settings/ExpertsSettings";
import { McpSettings } from "@/components/settings/McpSettings";
import { PluginsSettings } from "@/components/settings/PluginsSettings";
import { ProviderSettings } from "@/components/settings/ProviderSettings";
import { RefinementSettings } from "@/components/settings/RefinementSettings";
import { SkillsSettings } from "@/components/settings/SkillsSettings";
import { StewardSettings } from "@/components/settings/StewardSettings";
import { WorkspaceSettings } from "@/components/settings/WorkspaceSettings";
import { useAppStore } from "@/stores/app-store";
import { FolderOpen } from "lucide-react";

/** 每软件 APP 的专属设置页（AppSettings 按 appId 渲染） */
export function AppSettingsChat() {
  return <AppSettings appId="chat" />;
}
export function AppSettingsCoding() {
  return <AppSettings appId="coding" />;
}

export type AppId = "chat" | "coding" | "wiki" | "settings";

export interface AppEntry {
  id: AppId;
  /** i18n key：应用显示名 */
  nameKey: string;
  icon: ReactNode;
  component: ComponentType;
}

/**
 * 应用注册表（表内顺序即导航顺序）。
 * 内容组件全部是既有成熟组件零改动嵌入；壳层（导航/主面板）只认本表。
 */
export const APPS: Record<AppId, AppEntry> = {
  chat: {
    id: "chat",
    nameKey: "desktop.app.chat",
    icon: <MessageSquare size={18} />,
    component: ChatAppView,
  },
  coding: {
    id: "coding",
    nameKey: "desktop.app.coding",
    icon: <Code2 size={18} />,
    // M2：独立壳（文件树/编辑器/分支图 + 活任务清单）
    component: CodingApp,
  },
  wiki: {
    id: "wiki",
    nameKey: "desktop.app.wiki",
    icon: <BookOpen size={18} />,
    // WIKI 应用（xu-wiki 迁移 · bm-wiki 引擎）：三栏（树/阅读编辑/关系对话）
    component: WikiApp,
  },
  settings: {
    id: "settings",
    nameKey: "desktop.app.settings",
    icon: <Settings size={18} />,
    component: SettingsAppView,
  },
};

/**
 * 聊天应用 = 可停靠视图容器（v0.23 布局系统）：默认布局 = 左会话列表/中对话
 * （DEFAULT_LAYOUTS.chat 声明），用户可拖拽/叠放/关闭视图，布局自动持久化，
 * 导航右键「重置布局」恢复默认。
 */
function ChatAppView() {
  return (
    <div className="h-full min-w-0 bg-background">
      <DockLayout appId="chat" />
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
      {/* 设置侧栏宽度：2026-08-16 用户定调（+1/4 = 16.25rem） */}
      <div className="w-[16.25rem] shrink-0 overflow-y-auto border-r bg-background/60">
        <SettingsMenu />
      </div>
      <div className="min-w-0 flex-1">
        <SettingsPage />
      </div>
    </div>
  );
}

/**
 * WIKI 占位已退役（2026-08-16 WIKI 应用上线，见 components/wiki/WikiApp）；
 * WikiPlaceholder 组件与 desktop.app.wikiDesc 建设中文案一并删除。
 */

export type SettingsTab =
  | "app-chat"
  | "app-coding"
  | "experts"
  | "logs"
  | "appearance"
  | "mcp"
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
  /** 设置菜单分组（设置架构 2026-08-16）：app = 每软件 APP 设置；system = 全局设置 */
  group?: "app" | "system";
  /** 设置分级（设置架构 §十）：expert 项只在资深模式显示（默认 basic 可见） */
  tier?: "expert";
}

/** 设置页注册表（表内顺序即设置菜单顺序；新增 SettingsTab 必须在此登记） */
export const SETTINGS: Record<SettingsTab, SettingsEntry> = {
  // ── 应用组：每软件 APP 的专属设置（设置架构 2026-08-16）──
  "app-chat": {
    labelKey: "settings.menu.appChat",
    descKey: "settings.menu.appChatDesc",
    icon: <MessageSquare size={16} />,
    component: AppSettingsChat,
    group: "app",
  },
  "app-coding": {
    labelKey: "settings.menu.appCoding",
    descKey: "settings.menu.appCodingDesc",
    icon: <Code2 size={16} />,
    component: AppSettingsCoding,
    group: "app",
  },
  // ── 系统组：全局设置 ──
  experts: {
    labelKey: "settings.menu.experts",
    descKey: "settings.menu.expertsDesc",
    icon: <Users size={16} />,
    component: ExpertsSettings,
  },
  logs: {
    labelKey: "settings.menu.logs",
    descKey: "settings.menu.logsDesc",
    icon: <ScrollText size={16} />,
    component: LogsSettings,
  },
  mcp: {
    labelKey: "settings.menu.mcp",
    descKey: "settings.menu.mcpDesc",
    icon: <Cable size={16} />,
    component: McpSettings,
  },
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
    tier: "expert",
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

/** 设置主面板：按当前 tab 渲染对应设置页（带滚动容器）。
 * 2026-08-16 设计定调：标准/资深切换器移入「关于」页（不易找到、
 * 开启即锁定），此处不再显示；滚动容器加底部留白，防最后内容
 * 被状态栏遮挡。 */
export function SettingsPage() {
  const settingsTab = useAppStore((s) => s.settingsTab);
  const { component: Page } = SETTINGS[settingsTab];
  return (
    <div className="h-full min-w-0 overflow-y-auto bg-background pb-10">
      <div className="mx-auto max-w-3xl px-6 py-6">
        <Page />
      </div>
    </div>
  );
}
