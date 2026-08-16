/**
 * 经典软件界面（原三栏壳形态回归）——双 DE 并存验证（架构 §四·B：前端壳
 * 多套并存、内核只提供 API + 事件流）。
 *
 * 布局：左侧 48px 导航条 = **软件导航**（顶部 = 软件应用：对话/编程占位，
 * 未来 wiki 等；底部独立区 = 设置入口 + 桌面模式）——与旧 NavBar 的
 * bottom 分区语义一致（设置不是"软件"，是系统入口；插件/管家/模型提供商
 * 等设置项在设置应用二级菜单里）。
 * + 主面板（渲染 APPS[activeNav].component，内容组件零改动）
 * + 底部状态栏（共享 StatusBar，浅色跟随主题）。
 * 与后端共享同一 store（会话/消息/配置）。
 *
 * 桌面形态已退役（2026-08-16，用户拍板：全删除，留切换开关占位）。
 *
 * 应用内容区 = DockLayout 可停靠视图容器（v0.23）：导航图标右键菜单提供
 * 「重置布局」（只对有默认布局声明的应用显示，恢复该应用布局快照为默认）。
 */
import { useEffect, useState } from "react";
import type { AppId } from "@/lib/app-registry";
import { useTranslation } from "react-i18next";
import { LayoutPanelLeft, Settings } from "lucide-react";
import { APPS } from "@/lib/app-registry";
import { hasDockLayout } from "@/lib/dock-views";
import { resetDockLayout } from "@/components/layout/DockLayout";
import { useAppStore } from "@/stores/app-store";
import { StatusBar } from "@/components/shared/StatusBar";
import { cn } from "@/lib/utils";

/** 软件导航顺序（顶部区）：对话 + 编程 + wiki（占位）；新软件在此登记 */
const NAV_APPS: AppId[] = ["chat", "coding", "wiki"];
/** 占位应用（未立项）：导航置灰点不了（wiki 现状） */
const PLACEHOLDER_APPS: AppId[] = ["wiki"];

/** 导航图标右键菜单（当前仅「重置布局」一项） */
interface NavContextMenu {
  x: number;
  y: number;
  appId: AppId;
}

export function ClassicShell() {
  const { t } = useTranslation();
  const activeNav = useAppStore((s) => s.activeNav);
  const setActiveNav = useAppStore((s) => s.setActiveNav);
  const activateApp = useAppStore((s) => s.activateApp);
  const Page = APPS[activeNav].component;
  const [ctxMenu, setCtxMenu] = useState<NavContextMenu | null>(null);

  // 切到有会话场景的应用（chat/coding）：把聚焦会话切到该场景最近使用的会话
  // （一软件一会话，架构 §四·B 补充；无该场景会话时保持现状，由应用内引导创建）
  const switchTo = (id: AppId) => {
    setActiveNav(id);
    if (id === "chat" || id === "coding") void activateApp(id);
  };

  // 右键菜单关闭：点击任意处 / Esc / 滚动
  useEffect(() => {
    if (!ctxMenu) return;
    const close = () => setCtxMenu(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", onKey);
    window.addEventListener("wheel", close, { passive: true });
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("wheel", close);
    };
  }, [ctxMenu]);

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
      <div className="flex min-h-0 flex-1">
        {/* 左侧导航条：顶部=软件导航（对话/编程）；底部=设置 + 桌面模式 */}
        <nav
          aria-label={t("desktop.classicNav")}
          className="flex w-12 shrink-0 flex-col items-center gap-1 border-r border-border bg-muted/40 py-2"
        >
          {NAV_APPS.map((id) => {
            const app = APPS[id];
            const active = activeNav === id;
            const placeholder = PLACEHOLDER_APPS.includes(id);
            return (
              <button
                key={id}
                type="button"
                title={placeholder ? `${t(app.nameKey)}（${t("common.comingSoon")}）` : t(app.nameKey)}
                aria-label={t(app.nameKey)}
                aria-disabled={placeholder || undefined}
                disabled={placeholder}
                onClick={() => switchTo(id)}
                onContextMenu={
                  hasDockLayout(id)
                    ? (e) => {
                        e.preventDefault();
                        setCtxMenu({ x: e.clientX, y: e.clientY, appId: id });
                      }
                    : undefined
                }
                className={cn(
                  "flex h-9 w-9 items-center justify-center rounded-lg transition-colors",
                  active
                    ? "bg-primary/10 text-primary"
                    : "text-muted-foreground hover:bg-muted hover:text-foreground",
                  placeholder && "cursor-not-allowed opacity-40 hover:bg-transparent hover:text-muted-foreground",
                )}
              >
                {app.icon}
              </button>
            );
          })}

          {/* 底部独立区：设置入口（系统级，非软件）；桌面模式入口已并入
              设置→外观（形态切换卡片），导航条不再放 */}
          <div className="mt-auto flex flex-col items-center gap-1 border-t border-border pt-2">
            <button
              type="button"
              aria-label={t(APPS.settings.nameKey)}
              title={t(APPS.settings.nameKey)}
              onClick={() => switchTo("settings")}
              className={cn(
                "flex h-9 w-9 items-center justify-center rounded-lg transition-colors",
                activeNav === "settings"
                  ? "bg-primary/10 text-primary"
                  : "text-muted-foreground hover:bg-muted hover:text-foreground",
              )}
            >
              <Settings size={17} />
            </button>
          </div>
        </nav>

        {/* 主面板：按当前导航渲染应用内容（应用组件自带滚动/布局语义） */}
        <main className="min-w-0 flex-1">
          <Page />
        </main>
      </div>

      <StatusBar />

      {/* 导航右键菜单：重置该应用的可停靠布局 */}
      {ctxMenu && (
        <div
          role="menu"
          className="fixed z-[70] min-w-44 rounded-lg border border-border bg-popover p-1 shadow-lg"
          style={{ left: ctxMenu.x, top: ctxMenu.y }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          <div className="px-2.5 py-1.5 text-xs font-medium text-muted-foreground">
            {t(APPS[ctxMenu.appId].nameKey)}
          </div>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              resetDockLayout(ctxMenu.appId);
              setCtxMenu(null);
            }}
            className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-sm text-foreground hover:bg-accent"
          >
            <LayoutPanelLeft size={14} className="text-muted-foreground" />
            {t("dock.resetLayout")}
          </button>
        </div>
      )}
    </div>
  );
}
