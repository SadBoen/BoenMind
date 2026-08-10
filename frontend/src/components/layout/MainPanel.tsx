/**
 * 主面板：随导航切换内容（聊天窗口 / 设置页）。
 */
import { useAppStore } from "@/stores/app-store";
import { ChatWindow } from "@/components/chat/ChatWindow";
import { AppearanceSettings } from "@/components/settings/AppearanceSettings";
import { ProviderSettings } from "@/components/settings/ProviderSettings";
import { WorkspaceSettings } from "@/components/settings/WorkspaceSettings";
import { PluginsSettings } from "@/components/settings/PluginsSettings";
import { AboutSettings } from "@/components/settings/AboutSettings";

export function MainPanel() {
  const activeNav = useAppStore((s) => s.activeNav);
  const settingsTab = useAppStore((s) => s.settingsTab);

  if (activeNav === "chat") return <ChatWindow />;
  if (activeNav === "settings") {
    return (
      <div className="h-full min-w-0 overflow-y-auto bg-background">
        <div className="mx-auto max-w-3xl px-6 py-6">
          {settingsTab === "appearance" && <AppearanceSettings />}
          {settingsTab === "providers" && <ProviderSettings />}
          {settingsTab === "workspace" && <WorkspaceSettings />}
          {settingsTab === "plugins" && <PluginsSettings />}
          {settingsTab === "about" && <AboutSettings />}
        </div>
      </div>
    );
  }
  return (
    <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
      即将推出
    </div>
  );
}
