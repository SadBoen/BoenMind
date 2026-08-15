/**
 * 经典软件界面（原三栏壳形态回归）——双 DE 并存验证（架构 §四·B：前端壳
 * 多套并存、内核只提供 API + 事件流）。
 *
 * 布局：左侧 48px 导航条 = **软件导航**（顶部 = 软件应用：对话/编程占位，
 * 未来 wiki 等；底部独立区 = 设置入口 + 桌面模式）——与旧 NavBar 的
 * bottom 分区语义一致（设置不是"软件"，是系统入口；插件/管家/模型提供商
 * 等设置项在设置应用二级菜单里）。
 * + 主面板（渲染 APPS[activeNav].component，内容组件零改动）
 * + 底部状态栏（复用桌面壳 StatusBar，浅色变体）。
 * 与桌面壳共享同一 store（会话/消息/配置），后端零改动。
 *
 * 默认界面 = 本壳（用户拍板：软件形式优先）；桌面模式从导航条底部入口进入。
 */
import { useTranslation } from "react-i18next";
import { Monitor, Settings } from "lucide-react";
import { APPS, type AppId } from "@/lib/app-registry";
import { useAppStore } from "@/stores/app-store";
import { StatusBar } from "@/components/desktop/StatusBar";
import { cn } from "@/lib/utils";

/** 软件导航顺序（顶部区）：对话 + 编程（占位）；新软件（wiki 等）在此登记 */
const NAV_APPS: AppId[] = ["chat", "coding"];

export function ClassicShell() {
  const { t } = useTranslation();
  const activeNav = useAppStore((s) => s.activeNav);
  const setActiveNav = useAppStore((s) => s.setActiveNav);
  const setViewMode = useAppStore((s) => s.setViewMode);
  const Page = APPS[activeNav].component;

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
      <div className="flex min-h-0 flex-1">
        {/* 左侧导航条：顶部=软件导航（编程为占位——M2 前"点不了"）；底部=设置 + 桌面模式 */}
        <nav
          aria-label={t("desktop.classicNav")}
          className="flex w-12 shrink-0 flex-col items-center gap-1 border-r border-border bg-muted/40 py-2"
        >
          {NAV_APPS.map((id) => {
            const app = APPS[id];
            const active = activeNav === id;
            const isCoding = id === "coding";
            return (
              <button
                key={id}
                type="button"
                title={isCoding ? `${t(app.nameKey)}（${t("desktop.codingComingSoon")}）` : t(app.nameKey)}
                aria-label={t(app.nameKey)}
                aria-disabled={isCoding || undefined}
                // 编程应用占位：功能未实现前不可点（M2 编程应用上线后接线）
                disabled={isCoding}
                onClick={() => setActiveNav(id)}
                className={cn(
                  "flex h-9 w-9 items-center justify-center rounded-lg transition-colors",
                  active
                    ? "bg-primary/10 text-primary"
                    : "text-muted-foreground hover:bg-muted hover:text-foreground",
                  isCoding && "cursor-not-allowed opacity-40 hover:bg-transparent hover:text-muted-foreground",
                )}
              >
                {app.icon}
              </button>
            );
          })}

          {/* 底部独立区：设置入口（系统级，非软件）+ 桌面模式 */}
          <div className="mt-auto flex flex-col items-center gap-1 border-t border-border pt-2">
            <button
              type="button"
              aria-label={t(APPS.settings.nameKey)}
              title={t(APPS.settings.nameKey)}
              aria-disabled={activeNav === "settings" || undefined}
              onClick={() => setActiveNav("settings")}
              className={cn(
                "flex h-9 w-9 items-center justify-center rounded-lg transition-colors",
                activeNav === "settings"
                  ? "bg-primary/10 text-primary"
                  : "text-muted-foreground hover:bg-muted hover:text-foreground",
              )}
            >
              <Settings size={17} />
            </button>
            <button
              type="button"
              aria-label={t("desktop.switchDesktop")}
              title={t("desktop.switchDesktop")}
              onClick={() => setViewMode("desktop")}
              className="flex h-9 w-9 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            >
              <Monitor size={17} />
            </button>
          </div>
        </nav>

        {/* 主面板：按当前导航渲染应用内容（应用组件自带滚动/布局语义） */}
        <main className="min-w-0 flex-1">
          <Page />
        </main>
      </div>

      <StatusBar variant="classic" />
    </div>
  );
}
