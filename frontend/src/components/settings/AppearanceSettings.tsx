/**
 * 外观设置：亮色 / 暗色 / 跟随系统。
 */
import { useTheme } from "next-themes";
import { Laptop, Moon, Sun } from "lucide-react";
import { useAppStore } from "@/stores/app-store";
import { toast } from "sonner";

const THEMES = [
  { key: "light", label: "亮色", icon: <Sun size={16} /> },
  { key: "dark", label: "暗色", icon: <Moon size={16} /> },
  { key: "system", label: "跟随系统", icon: <Laptop size={16} /> },
] as const;

export function AppearanceSettings() {
  const { theme, setTheme } = useTheme();
  const config = useAppStore((s) => s.config);
  const saveConfig = useAppStore((s) => s.saveConfig);

  const applyTheme = async (key: string) => {
    setTheme(key);
    if (config) {
      try {
        await saveConfig({ ...config, theme: key });
        toast.success("外观已更新");
      } catch (err) {
        toast.error(`保存失败: ${String(err)}`);
      }
    }
  };

  return (
    <section className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">外观</h2>
        <p className="text-sm text-muted-foreground">选择 BoenMind 的主题模式</p>
      </div>

      <div className="grid grid-cols-3 gap-3">
        {THEMES.map((t) => (
          <button
            key={t.key}
            type="button"
            onClick={() => void applyTheme(t.key)}
            className={`flex flex-col items-center gap-2 rounded-xl border-2 p-4 transition-colors ${
              theme === t.key
                ? "border-primary bg-primary/5"
                : "border-border hover:border-muted-foreground/40"
            }`}
          >
            {t.icon}
            <span className="text-sm font-medium">{t.label}</span>
          </button>
        ))}
      </div>

      <p className="text-xs text-muted-foreground">
        主题选择同时保存在后端配置中，桌面端与网页端一致。
      </p>
    </section>
  );
}
