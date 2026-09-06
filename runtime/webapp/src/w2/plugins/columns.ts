//! 插件中心表格列配置(自 PluginsPage.tsx 机械移入)。
import { STORAGE_KEYS, storage } from "@/lib/storage";

export type ColKey = "name" | "category" | "tools" | "actions";

export interface ColConfig {
  key: ColKey;
  label: string;
  defaultWidth: number;
  minWidth: number;
  align?: "left" | "center" | "right";
}

export const TABLE_COLUMNS: ColConfig[] = [
  { key: "name", label: "名称 / 标识", defaultWidth: 220, minWidth: 160, align: "left" },
  { key: "category", label: "类别", defaultWidth: 110, minWidth: 90, align: "left" },
  { key: "tools", label: "提供工具", defaultWidth: 260, minWidth: 180, align: "left" },
  { key: "actions", label: "操作", defaultWidth: 180, minWidth: 150, align: "center" },
];

export function loadColWidths(): Record<ColKey, number> {
  const defaults: Record<ColKey, number> = {
    name: 220,
    category: 110,
    tools: 260,
    actions: 180,
  };
  const raw = storage.get(STORAGE_KEYS.PLUGINS_TABLE_COLS);
  if (!raw) return defaults;
  try {
    const parsed = JSON.parse(raw);
    return {
      name: typeof parsed.name === "number" ? Math.max(160, parsed.name) : defaults.name,
      category: typeof parsed.category === "number" ? Math.max(90, parsed.category) : defaults.category,
      tools: typeof parsed.tools === "number" ? Math.max(180, parsed.tools) : defaults.tools,
      actions: typeof parsed.actions === "number" ? Math.max(150, parsed.actions) : defaults.actions,
    };
  } catch {
    return defaults;
  }
}

