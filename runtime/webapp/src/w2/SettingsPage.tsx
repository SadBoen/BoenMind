// W2 设置中心:整页式(左侧设置导航 + 右内容区),齿轮进入。
// 导航:常规 / 模型提供商 / 插件 / MCP / 外观(W3 主题系统)+
// 插件 PIN 快捷项(PIN 后在此显示,点击跳插件页并按名筛选)。
import { useEffect, useState } from "react";
// 导航:常规 / 模型 / 插件 / 外观 / 角色 / 日志 / 关于
import { BoxIcon, InfoIcon, PlugIcon, ScrollTextIcon, SlidersHorizontalIcon, SparklesIcon, WrenchIcon } from "lucide-react";
import { ProvidersPage } from "./ProvidersPage";
import { PluginsPage } from "./PluginsPage";
import { RolesPage } from "./RolesPage";
import { LogsPage } from "./LogsPage";
import { AboutPage } from "./AboutPage";
import { GeneralPage } from "./GeneralPage";
import { AppearancePage } from "@/w3/AppearancePage";
import { cn } from "@/lib/utils";

type Section = "general" | "providers" | "plugins" | "appearance" | "roles" | "logs" | "about";

export function SettingsPage({ onClose }: { onClose: () => void }) {
  const [section, setSection] = useState<Section>("providers");
  const [pluginFilter, setPluginFilter] = useState<string | undefined>(undefined);
  const [pluginEditTarget, setPluginEditTarget] = useState<string | null>(null);

  const goPluginWithFilter = (name: string) => {
    setPluginFilter(name);
    setSection("plugins");
  };

  // 图标栏常驻,齿轮即开关;Esc 也可关闭(用户裁定:返回按钮不再需要)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    // 从 rail(52px 图标栏)右侧起铺满:设置打开时图标栏常驻不消失(用户裁定)
    <div
      className="bg-background fixed inset-y-0 right-0 left-[52px] z-[var(--z-settings,40)] flex flex-col border-l"
      data-slot="settings-page"
    >
      <div className="border-b flex items-center gap-3 px-4 py-3">
        <span className="text-[15px] font-semibold">设置</span>
        <span className="text-muted-foreground text-[12px]">齿轮可开关 · Esc 关闭</span>
        <span className="flex-1" />
      </div>

      <div className="flex min-h-0 flex-1">
        <nav className="bg-panel w-56 shrink-0 overflow-y-auto border-r p-3">
          <NavItem
            active={section === "general"}
            onClick={() => setSection("general")}
            icon={<WrenchIcon className="size-4" />}
            label="常规"
          />
          <NavItem
            active={section === "providers"}
            onClick={() => setSection("providers")}
            icon={<SlidersHorizontalIcon className="size-4" />}
            label="模型"
          />
          <NavItem
            active={section === "plugins"}
            onClick={() => {
              setPluginFilter(undefined);
              setSection("plugins");
            }}
            icon={<PlugIcon className="size-4" />}
            label="插件"
          />
          <NavItem
            active={section === "appearance"}
            onClick={() => setSection("appearance")}
            icon={<SparklesIcon className="size-4" />}
            label="外观"
          />
          <NavItem
            active={section === "roles"}
            onClick={() => setSection("roles")}
            icon={<BoxIcon className="size-4" />}
            label="角色"
          />
          <NavItem
            active={section === "logs"}
            onClick={() => setSection("logs")}
            icon={<ScrollTextIcon className="size-4" />}
            label="日志"
          />
          <NavItem
            active={section === "about"}
            onClick={() => setSection("about")}
            icon={<InfoIcon className="size-4" />}
            label="关于"
          />
        </nav>

        <main className="min-w-0 flex-1 overflow-y-auto p-6" data-slot="settings-content">
          <div className="mx-auto max-w-4xl">
            {section === "general" ? <GeneralPage /> : null}
            {section === "providers" ? <ProvidersPage /> : null}
            {section === "plugins" ? (
              <PluginsPage
                initialFilter={pluginFilter}
                editTarget={pluginEditTarget}
                onConsumedEditTarget={() => setPluginEditTarget(null)}
              />
            ) : null}
            {section === "appearance" ? <AppearancePage /> : null}
            {section === "roles" ? <RolesPage /> : null}
            {section === "logs" ? <LogsPage /> : null}
            {section === "about" ? <AboutPage /> : null}
          </div>
        </main>
      </div>
    </div>
  );
}

function NavItem({
  active,
  onClick,
  icon,
  label,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
}) {
  return (
    <button
      className={cn(
        "mb-0.5 flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-[13px] transition-colors",
        active ? "bg-accent text-accent-foreground font-medium" : "hover:bg-accent/60",
      )}
      onClick={onClick}
    >
      {icon}
      <span className="flex-1">{label}</span>
    </button>
  );
}
