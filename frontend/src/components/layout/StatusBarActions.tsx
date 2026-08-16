/**
 * 面板标题栏右侧统一状态区（用户拍板"把 X 关闭那一行做成状态栏"，2026-08-15）：
 * dockview 8.1 header action 槽位（rightHeaderActionsComponent）——每个面板组
 * 标题栏右侧挂本组件，按组内活跃面板渲染状态。
 *
 * 状态项：会话 token 用量已迁入输入框圆环（TokenRing，2026-08-17 用户拍板），
 * 本组件不再渲染右上角用量按钮。当前仅剩 "+" 视图菜单（面板关闭后的重开入口）。
 */
import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { Menu, Plus } from "lucide-react";
import type { IDockviewHeaderActionsProps } from "dockview-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/stores/app-store";
import { appIdOf, filterOpenable, layoutReopenItems, openPanel } from "@/lib/dock-open";
import { VIEWS } from "@/lib/dock-views";

/**
 * 标题栏"+"视图菜单（2026-08-15 用户"面板关闭后没有二次打开方法"补口）：
 * 每个面板组标题栏右侧的 + 按钮 → 该应用可重开视图列表，点击重开
 * （默认位置优先，否则当前组叠 Tab）。dockview 8.1 关闭组内最后一个
 * 面板会移除空组，watermark 只覆盖全关场景，日常入口在此。
 */
function AddPanelMenu({ api }: { api: IDockviewHeaderActionsProps["containerApi"] }) {
  const { t } = useTranslation();
  const appId = appIdOf(api);
  const [open, setOpen] = useState(false);
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);
  const [, setTick] = useState(0);

  // 打开/关闭面板后刷新可开列表（对话视图单实例：已打开则不再列出）
  useEffect(() => {
    const disposable = api.onDidLayoutChange(() => setTick((x) => x + 1));
    return () => disposable.dispose();
  }, [api]);

  const items = useMemo(
    () => (appId ? filterOpenable(layoutReopenItems(appId, (v) => t(VIEWS[v].titleKey)), api) : []),
    [appId, api, t],
  );

  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        className="h-6 w-6 px-0 text-muted-foreground"
        title={t("dock.addView")}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setAnchor({ x: r.right, y: r.bottom });
          setOpen((o) => !o);
        }}
      >
        <Plus size={12} />
      </Button>
      {open && anchor && (
        createPortal(
          <>
            <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
            <div
              className="fixed z-50 w-44 rounded-lg border bg-popover p-1.5 shadow-md"
              style={{ right: Math.min(window.innerWidth - anchor.x + 8, 16), top: anchor.y + 6 }}
            >
              {items.map((item) => (
                <button
                  key={item.view}
                  type="button"
                  onClick={() => {
                    openPanel(api, undefined, item);
                    setOpen(false);
                  }}
                  className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-accent"
                >
                  <Plus size={12} className="shrink-0 text-muted-foreground" />
                  {item.title}
                </button>
              ))}
            </div>
          </>,
          document.body,
        )
      )}
    </>
  );
}

/**
 * 统一状态区宿主：按组内活跃面板的视图类型渲染状态项。
 * 当前仅 "+" 视图菜单（面板关闭后的重开入口）。token 用量已迁入输入框圆环。
 */
export function StatusBarActions({ containerApi }: IDockviewHeaderActionsProps) {
  return <AddPanelMenu api={containerApi} />;
}

/**
 * 状态栏最左槽位（prefix）：聊天单元内嵌会话列表的显隐开关。
 * 用户拍板"列表在聊天单元内部，由顶部状态栏控制"（2026-08-15）——
 * 三横按钮与 token 状态、关闭按钮同一行（状态栏统一承载）。
 * 仅对话面板组显示；按场景（chat/coding）记忆显隐，聊天应用默认展开。
 */
export function SessionsToggle({ activePanel }: IDockviewHeaderActionsProps) {
  const { t } = useTranslation();
  const params = (activePanel?.params ?? {}) as { app?: string };
  const scene = params.app ?? "chat";
  const open = useAppStore((s) => s.chatSessionsOpen[scene] ?? scene === "chat");
  const toggle = useAppStore((s) => s.toggleChatSessions);
  if (activePanel?.view.contentComponent !== "chat-pane") return null;
  return (
    <Button
      variant="ghost"
      size="sm"
      className={cn(
        "h-7 w-9 px-0 text-muted-foreground",
        open && "bg-accent text-foreground",
      )}
      title={t("chat.sessions")}
      onClick={() => toggle(scene)}
    >
      <Menu size={14} />
    </Button>
  );
}
