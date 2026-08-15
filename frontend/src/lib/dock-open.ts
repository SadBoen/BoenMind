/**
 * 视图重开共享逻辑（2026-08-15 用户"面板关闭后没有二次打开方法"补口）：
 *
 * dockview 8.1 关闭组内最后一个面板会移除空组（无保留空组选项），watermark
 * 只能覆盖"全部面板关闭"场景；日常"关一个面板"的入口 = 面板标题栏 "+" 菜单。
 * 两处入口共用本模块的清单计算（默认布局视图 + 编辑器）与打开逻辑
 * （id 去重 / 位置 = 默认布局参照面板仍在 → 原位置，否则当前组）。
 */
import type { DockviewApi } from "dockview-react";
import { DEFAULT_LAYOUTS, VIEWS, type DockPanelSpec, type ViewId } from "./dock-views";
import type { AppId } from "./app-registry";

/** 可重开视图项：默认布局声明（id/params/位置/尺寸）+ 标题 */
export interface ReopenItem extends DockPanelSpec {
  view: ViewId;
  title: string;
}

/** 计算应用可重开视图清单：默认布局里的视图（去重）+ 编辑器（可多开，随时加回） */
export function layoutReopenItems(appId: AppId, titleOf: (view: ViewId) => string): ReopenItem[] {
  const layout = DEFAULT_LAYOUTS[appId];
  if (!layout) return [];
  const seen = new Set<ViewId>();
  const items: ReopenItem[] = [];
  for (const spec of layout.panels) {
    if (seen.has(spec.view)) continue;
    seen.add(spec.view);
    items.push({ ...spec, title: titleOf(spec.view) });
  }
  if (!seen.has("editor")) items.push({ id: "editor", view: "editor", title: titleOf("editor") });
  return items;
}

/** 对话视图单实例（绑定场景）：已打开则不再重复提供；其余视图可多开，常显 */
export function filterOpenable(items: ReopenItem[], api: DockviewApi): ReopenItem[] {
  const chatOpen = api.panels.some((p) => p.view.contentComponent === "chat-pane");
  return items.filter((it) => it.view !== "chat-pane" || !chatOpen);
}

/**
 * 打开视图：id 复用默认声明（被占用则自动加序号后缀多开）；
 * 位置 = 默认布局参照面板仍在 → 原位置；否则 → 传入的组 id（watermark 的
 * 空组）或当前活动组（标题栏菜单）。
 */
export function openPanel(api: DockviewApi, groupId: string | undefined, item: ReopenItem) {
  let id = item.id;
  let n = 2;
  while (api.getPanel(id)) id = `${item.id}-${n++}`;
  const position = item.position && api.getPanel(item.position.reference)
    ? { referencePanel: item.position.reference, direction: item.position.direction }
    : groupId
      ? { referenceGroup: groupId }
      : undefined;
  api.addPanel({
    id,
    component: item.view,
    title: item.title,
    params: item.params,
    initialWidth: item.initialWidth,
    initialHeight: item.initialHeight,
    position,
  });
}

/** DockviewApi → 应用 id 注册表（DockLayout 挂载时登记；header action 组件反查所属应用） */
const apiAppMap = new WeakMap<DockviewApi, AppId>();

export function registerApiApp(api: DockviewApi, appId: AppId) {
  apiAppMap.set(api, appId);
}

export function unregisterApiApp(api: DockviewApi) {
  apiAppMap.delete(api);
}

export function appIdOf(api: DockviewApi): AppId | undefined {
  return apiAppMap.get(api);
}

export { VIEWS };
