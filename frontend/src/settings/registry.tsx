import { lazy, type ComponentType, type LazyExoticComponent, type ReactNode } from "react";
import { IconChat, IconCode, IconGear, IconStar, IconUser, IconWiki } from "../lib/icons";

export interface SettingsSection {
  id: string;
  label: string;
  icon?: ReactNode;
  order?: number;
  component: LazyExoticComponent<ComponentType>;
}

const sections: SettingsSection[] = [
  { id: "general", label: "通用", icon: <IconGear />, order: 10, component: lazy(() => import("./GeneralSection")) },
  { id: "model", label: "模型", icon: <IconChat />, order: 20, component: lazy(() => import("./ModelSection")) },
  { id: "appearance", label: "外观", icon: <IconStar />, order: 30, component: lazy(() => import("./AppearanceSection")) },
  { id: "account", label: "账号", icon: <IconUser />, order: 40, component: lazy(() => import("./AccountSection")) },
  { id: "skill", label: "技能", icon: <IconWiki />, order: 50, component: lazy(() => import("./SkillSection")) },
  { id: "plugin", label: "插件", icon: <IconCode />, order: 60, component: lazy(() => import("./PluginSection")) },
];

export function getSections(): SettingsSection[] {
  return [...sections].sort((a, b) => (a.order ?? 100) - (b.order ?? 100));
}
