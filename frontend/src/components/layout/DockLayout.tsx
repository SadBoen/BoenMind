/**
 * DockLayout —— 可停靠视图容器宿主组件（架构 §四·B 补充 2，v0.23）。
 *
 * VS Code workbench 模型的应用内容区：视图可停靠/悬浮/叠放 Tab/关闭/最大化、
 * 分界线拖拽；每应用一份默认布局 + 布局快照持久化（localStorage）+ 重置入口。
 *
 * 用法：应用内容区 = `<DockLayout appId="coding" ref={...} />` 一行接入。
 * 视图在 lib/dock-views.tsx 登记（VIEWS = 宿主共享公共组件零改动嵌入，
 * DEFAULT_LAYOUTS = 每应用默认布局声明）；本组件只做实例化/持久化/重置，
 * 不感知任何应用业务（场景绑定等语义由视图参数 params 表达，见 chat-pane）。
 *
 * 视图实例语义（用户拍板）：对话视图单实例且绑定应用场景（复用 session.app
 * 机制）；终端/文件树/任务列表/编辑器可多开（dockview 原生支持，注册表
 * 不做单例限制）；专家团队模式（多模型并行）属模型层语义另行拍板。
 */
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef } from "react";
import type { FunctionComponent } from "react";
import { useTranslation } from "react-i18next";
import { useTheme } from "next-themes";
import {
  DockviewReact,
  type DockviewApi,
  type DockviewReadyEvent,
  type IDockviewPanelProps,
  type SerializedDockview,
} from "dockview-react";
import { DEFAULT_LAYOUTS, VIEWS, type ViewId } from "@/lib/dock-views";
import type { AppId } from "@/lib/app-registry";
import { cn } from "@/lib/utils";
import { SessionsToggle, StatusBarActions } from "./StatusBarActions";

export interface DockLayoutHandle {
  /** 重置为应用默认布局（导航右键/标题栏入口） */
  resetLayout: () => void;
}

interface DockLayoutProps {
  appId: AppId;
}

/**
 * 布局快照 key（每应用一份）。v2：DEFAULT_LAYOUTS 演进（新增 git-graph 视图）
 * 时 bump 版本——旧快照不包含新默认视图，版本化后重建默认布局。代价是
 * 用户自定义布局随版本重置一次；插件默认布局声明（§四·C）落地时再设计
 * 精细迁移（快照指纹对比），当前阶段默认布局即用户所见，bump 可接受。
 */
const layoutKey = (appId: AppId) => `boenmind.dock.v6.${appId}`;

/**
 * 布局重置注册表：DockLayout 实例挂载时登记，壳层（导航右键菜单/标题栏）
 * 通过 resetDockLayout(appId) 触发重置——避免把 ref 从壳层一路穿透到应用
 * 内容组件（应用组件 = 静态注册的 APPS[].component，无 ref 通道）。
 */
const dockHandles = new Map<AppId, DockLayoutHandle>();

function registerDockLayout(appId: AppId, handle: DockLayoutHandle) {
  dockHandles.set(appId, handle);
  return () => {
    if (dockHandles.get(appId) === handle) dockHandles.delete(appId);
  };
}

/** 重置指定应用的可停靠布局为默认（无实例时静默忽略） */
export function resetDockLayout(appId: AppId) {
  dockHandles.get(appId)?.resetLayout();
}

function debounce(fn: () => void, ms: number) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return () => {
    clearTimeout(timer);
    timer = setTimeout(fn, ms);
  };
}

export const DockLayout = forwardRef<DockLayoutHandle, DockLayoutProps>(function DockLayout(
  { appId },
  ref,
) {
  const { t } = useTranslation();
  const { resolvedTheme } = useTheme();
  const apiRef = useRef<DockviewApi | null>(null);
  /** fromJSON 恢复期间不触发落盘（恢复本身就是刚读出的快照，写回无意义） */
  const restoringRef = useRef(false);

  const saveLayout = useCallback(() => {
    const api = apiRef.current;
    if (!api || restoringRef.current) return;
    try {
      localStorage.setItem(layoutKey(appId), JSON.stringify(api.toJSON()));
    } catch {
      /* 快照序列化失败/存储满时忽略，不影响使用 */
    }
  }, [appId]);

  const buildDefaultLayout = useCallback(
    (api: DockviewApi) => {
      const layout = DEFAULT_LAYOUTS[appId];
      if (!layout) return;
      for (const spec of layout.panels) {
        api.addPanel({
          id: spec.id,
          component: spec.view,
          title: t(VIEWS[spec.view].titleKey),
          params: spec.params,
          initialWidth: spec.initialWidth,
          initialHeight: spec.initialHeight,
          position: spec.position
            ? { referencePanel: spec.position.reference, direction: spec.position.direction }
            : undefined,
        });
      }
    },
    [appId, t],
  );
  const resetLayout = useCallback(() => {
    const api = apiRef.current;
    if (!api) return;
    localStorage.removeItem(layoutKey(appId));
    restoringRef.current = true;
    api.clear();
    buildDefaultLayout(api);
    restoringRef.current = false;
    saveLayout();
  }, [appId, buildDefaultLayout, saveLayout]);

  useImperativeHandle(ref, () => ({ resetLayout }), [resetLayout]);

  // 挂载登记（导航右键「重置布局」入口），卸载注销
  useEffect(() => registerDockLayout(appId, { resetLayout }), [appId, resetLayout]);

  const onReady = useCallback(
    (event: DockviewReadyEvent) => {
      const api = event.api;
      apiRef.current = api;
      // 布局结构/尺寸变化（聚合事件：停靠/悬浮/叠放/分界线拖拽/最大化…）→ 防抖落盘
      const save = debounce(saveLayout, 500);
      api.onDidLayoutChange(() => save());

      // 恢复上次布局快照；无快照或快照损坏 → 构建应用默认布局
      const saved = localStorage.getItem(layoutKey(appId));
      if (saved) {
        try {
          restoringRef.current = true;
          api.fromJSON(JSON.parse(saved) as SerializedDockview);
          restoringRef.current = false;
          return;
        } catch {
          restoringRef.current = false;
        }
      }
      buildDefaultLayout(api);
    },
    [appId, buildDefaultLayout, saveLayout],
  );

  // 卸载前最后落一次盘（分界线拖拽后的比例在 unload 时留住）
  useEffect(() => () => saveLayout(), [saveLayout]);

  const components = useMemo(() => {
    const map: Record<string, FunctionComponent<IDockviewPanelProps>> = {};
    for (const id of Object.keys(VIEWS) as ViewId[]) map[id] = VIEWS[id].component;
    return map;
  }, []);

  return (
    <DockviewReact
      className={cn(
        "h-full w-full",
        resolvedTheme === "dark" ? "dockview-theme-dark" : "dockview-theme-light",
      )}
      components={components}
      onReady={onReady}
      rightHeaderActionsComponent={StatusBarActions}
      prefixHeaderActionsComponent={SessionsToggle}
    />
  );
});
