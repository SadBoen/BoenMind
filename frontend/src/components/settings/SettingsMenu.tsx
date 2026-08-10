/**
 * 设置二级菜单：外观 / 模型提供商 / 工作文件夹。
 */
import { FolderOpen, Info, MonitorCog, Palette, Puzzle } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAppStore, type SettingsTab } from "@/stores/app-store";

const MENU_ITEMS: { key: SettingsTab; label: string; icon: React.ReactNode; desc: string }[] = [
  { key: "appearance", label: "外观", icon: <Palette size={16} />, desc: "主题与显示" },
  { key: "providers", label: "模型提供商", icon: <MonitorCog size={16} />, desc: "模型与 API 配置" },
  { key: "workspace", label: "工作文件夹", icon: <FolderOpen size={16} />, desc: "默认文件浏览目录" },
  { key: "plugins", label: "插件", icon: <Puzzle size={16} />, desc: "扩展与工具" },
  { key: "about", label: "关于", icon: <Info size={16} />, desc: "版本与更新" },
];

export function SettingsMenu() {
  const settingsTab = useAppStore((s) => s.settingsTab);
  const setSettingsTab = useAppStore((s) => s.setSettingsTab);

  return (
    <div className="flex flex-col gap-0.5 p-2">
      {MENU_ITEMS.map((item) => (
        <button
          key={item.key}
          type="button"
          onClick={() => setSettingsTab(item.key)}
          className={cn(
            "flex items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm transition-colors",
            settingsTab === item.key
              ? "bg-accent text-accent-foreground"
              : "hover:bg-accent/50",
          )}
        >
          {item.icon}
          <div className="min-w-0">
            <p className="font-medium leading-tight">{item.label}</p>
            <p className="text-[10px] text-muted-foreground">{item.desc}</p>
          </div>
        </button>
      ))}
    </div>
  );
}
