/**
 * 经典软件界面（原三栏壳形态回归）——双 DE 并存验证（架构 §四·B：前端壳
 * 多套并存、内核只提供 API + 事件流）。
 *
 * 布局：左侧 48px 导航条（应用图标 = APPS 注册表，点击切换主面板）
 *     + 主面板（渲染 APPS[activeNav].component，内容组件零改动）
 *     + 底部状态栏（复用桌面壳 StatusBar，浅色变体）。
 * 与桌面壳共享同一 store（会话/消息/配置），后端零改动。
 *
 * 默认界面 = 本壳（用户拍板：软件形式优先）；桌面模式从导航条底部入口进入。
 */
import { useTranslation } from "react-i18next";
import { Monitor } from "lucide-react";
import { APPS, APP_LIST } from "@/lib/app-registry";
import { useAppStore } from "@/stores/app-store";
import { StatusBar } from "@/components/desktop/StatusBar";
import { cn } from "@/lib/utils";

export function ClassicShell() {
  const { t } = useTranslation();
  const activeNav = useAppStore((s) => s.activeNav);
  const setActiveNav = useAppStore((s) => s.setActiveNav);
  const setViewMode = useAppStore((s) => s.setViewMode);
  const Page = APPS[activeNav].component;

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
      <div className="flex min-h-0 flex-1">
        {/* 左侧导航条：应用入口（编程为占位——M2 编程应用前"点不了"）+ 底部桌面模式入口 */}
        <nav
          aria-label={t("desktop.classicNav")}
          className="flex w-12 shrink-0 flex-col items-center gap-1 border-r border-border bg-muted/40 py-2"
        >
          {APP_LIST.map((app) => {
            const active = activeNav === app.id;
            const isCoding = app.id === "coding";
            return (
              <button
                key={app.id}
                type="button"
                title={isCoding ? `${t(app.nameKey)}（${t("desktop.codingComingSoon")}）` : t(app.nameKey)}
                aria-label={t(app.nameKey)}
                aria-disabled={isCoding || undefined}
                // 编程应用占位：功能未实现前不可点（M2 编程应用上线后接线）
                disabled={isCoding}
                onClick={() => setActiveNav(app.id)}
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

          {/* 底部：桌面模式（OS 形态）入口 */}
          <div className="mt-auto flex flex-col items-center gap-1 border-t border-border pt-2">
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
