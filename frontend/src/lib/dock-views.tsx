/**
 * 应用布局系统的视图注册表（架构 §四·B 补充 2，v0.23 —— VS Code workbench 模型）。
 *
 * 视图 = 可停靠/悬浮/叠放/关闭/最大化的可复用面板，全部登记在本表：
 * 对话/会话列表/终端/文件树/任务列表/编辑器，都是宿主共享的公共组件
 * （ChatPane/TerminalPane 等）零改动嵌入，dockview 面板只做壳层包裹。
 * 新公共组件挂载视图时在此登记一行即可。
 *
 * 视图实例语义（用户拍板）：对话视图单实例且绑定应用场景（复用 session.app
 * 机制——编程里的对话是编程专家，不会跑到 WIKI）；终端/文件树/任务列表/
 * 编辑器可多开（dockview 原生支持同一视图多面板，这里不做单例限制）。
 */
import { useEffect } from "react";
import type { IDockviewPanelProps } from "dockview-react";
import { ChatPane } from "@/components/chat/ChatPane";
import { SessionList } from "@/components/chat/SessionList";
import { FilePanel } from "@/components/files/FilePanel";
import { Editor } from "@/components/coding/Editor";
import { GitGraph } from "@/components/coding/GitGraph";
import { TodoPanel } from "@/components/coding/TodoPanel";
import { TerminalPane } from "@/components/terminal/TerminalPane";
import { useAppStore } from "@/stores/app-store";
import type { AppId } from "./app-registry";

/** 视图 id（同时是 dockview 面板的 component 名与默认布局的引用 key） */
export type ViewId =
  | "session-list"
  | "chat-pane"
  | "file-panel"
  | "editor"
  | "todo-panel"
  | "terminal"
  | "git-graph";

export interface DockViewEntry {
  /** i18n key：视图显示名（Tab 标题） */
  titleKey: string;
  /** 面板组件（可读 params；视图组件零改动，dockview 面板 props 在此消化） */
  component: (props: IDockviewPanelProps) => React.ReactNode;
}

/** 视图注册表（表内顺序即注册顺序，无布局语义） */
export const VIEWS: Record<ViewId, DockViewEntry> = {
  "session-list": { titleKey: "dock.view.sessionList", component: () => <SessionList /> },
  // 对话视图是宿主能力：形态与场景由面板 params 决定（chat 应用=full/chat，
  // 编程壳=panel/coding）。面板挂载即绑定场景会话（一软件一会话，聚焦会话
  // 永远属于当前场景——chat 场景幂等，coding 场景无会话则懒创建）。
  "chat-pane": {
    titleKey: "dock.view.chat",
    component: ChatPaneView,
  },
  // file-panel：编程壳的面板带 coding 标记（头部显示项目切换器 + git 状态）；
  // 聊天应用的工作目录面板不带（保持纯目录浏览）。
  "file-panel": {
    titleKey: "dock.view.files",
    component: (props) => <FilePanel coding={props.params?.coding === true} />,
  },
  editor: { titleKey: "dock.view.editor", component: () => <Editor /> },
  "todo-panel": { titleKey: "dock.view.tasks", component: () => <TodoPanel /> },
  terminal: { titleKey: "dock.view.terminal", component: ProjectTerminal },
  "git-graph": { titleKey: "dock.view.gitGraph", component: () => <GitGraph /> },
};

// oxlint-disable-next-line react/only-export-components —— 注册表文件：常量 + 包装组件
// 是注册表的本质（ChatPaneView 是面板参数→组件的适配器，不属于组件文件）
function ChatPaneView({ params }: IDockviewPanelProps) {
  const app = (params?.app as AppId) ?? "chat";
  const ensureAppSession = useAppStore((s) => s.ensureAppSession);
  useEffect(() => {
    void ensureAppSession(app);
  }, [app, ensureAppSession]);
  return <ChatPane variant={params?.variant === "panel" ? "panel" : "full"} scene={app} />;
}

/**
 * 终端视图（编程壳）：启动目录 = 当前项目根（无项目 = 后端配置工作目录兜底）。
 * 终端是长会话（切换项目不迁移已开终端——与真实 IDE 语义一致：会话属于
 * 打开时所在的目录；要新项目终端就新开/重开面板，视图支持多开）。
 */
function ProjectTerminal() {
  const projectRoot = useAppStore((s) => s.currentProject?.root);
  return <TerminalPane cwd={projectRoot ?? undefined} />;
}

/** 默认布局里一块面板的摆放声明 */
export interface DockPanelSpec {
  /** 面板唯一 id（同一应用布局内不可重复；可多开视图用不同 id 重复声明 view） */
  id: string;
  view: ViewId;
  /** 面板参数（透传给视图组件；序列化布局时随快照保存） */
  params?: Record<string, unknown>;
  /**
   * 摆放位置：第一块省略（居中占满）；其余须给 reference + direction。
   * direction 取值（dockview 语义）：left/right/above/below = 以 reference 面板为
   * 参照在对应方向开新组；within = 并入 reference 所在组叠 Tab。
   */
  position?: { reference: string; direction: "left" | "right" | "above" | "below" | "within" };
  initialWidth?: number;
  initialHeight?: number;
}

/** 应用默认布局（每应用一份；新应用有可停靠视图时在此声明） */
export const DEFAULT_LAYOUTS: Partial<Record<AppId, { panels: DockPanelSpec[] }>> = {
  // 编程壳（v5 重排，2026-08-15 用户拍板"不需要编辑器，一行代码都不用看"）：
  // 中=任务清单主区 / 左=文件树 / 右=对话独立列 / 底部=终端|分支图叠放。
  // 编辑器视图仍在注册表（可多开，用户随时可加回——功能单元模式）。
  coding: {
    panels: [
      { id: "todo-panel", view: "todo-panel" },
      {
        id: "file-panel",
        view: "file-panel",
        params: { coding: true },
        position: { reference: "todo-panel", direction: "left" },
        initialWidth: 240,
      },
      {
        id: "chat-pane",
        view: "chat-pane",
        params: { variant: "panel", app: "coding" },
        position: { reference: "todo-panel", direction: "right" },
        initialWidth: 320,
      },
      {
        id: "terminal",
        view: "terminal",
        position: { reference: "todo-panel", direction: "below" },
        initialHeight: 200,
      },
      {
        id: "git-graph",
        view: "git-graph",
        position: { reference: "terminal", direction: "within" },
      },
    ],
  },
  // 聊天应用（v6 重排，2026-08-15 用户"SESSION 在聊天单元内部"）：会话列表不再是
  // 独立 dock 面板——收进 chat-pane 单元（内嵌列表，状态栏三横按钮控制显隐，
  // 默认展开）；右=工作目录文件列表（随手翻文件）
  chat: {
    panels: [
      { id: "chat-pane", view: "chat-pane", params: { app: "chat" } },
      {
        id: "file-panel",
        view: "file-panel",
        position: { reference: "chat-pane", direction: "right" },
        initialWidth: 240,
      },
    ],
  },
};

/** 应用是否声明了可停靠布局（无声明则应用内容区按原样渲染） */
export function hasDockLayout(appId: AppId): boolean {
  return appId in DEFAULT_LAYOUTS;
}
