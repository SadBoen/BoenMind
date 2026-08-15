/**
 * 编程应用独立壳（M2 起，用户拍板"编程为软件实现第一优先"）。
 *
 * ┌───────────────────────────────────────┐
 * │ DockLayout（可停靠视图容器，v0.23）：    │
 * │ 默认布局 = 左文件树 / 中任务清单 /        │
 * │ 右对话 / 底部分支图|终端 叠放 Tab        │
 * └───────────────────────────────────────┘
 * 后端零新增编排概念：文件走 /api/workspace（读/写），清单走事件日志
 * todo/write 投影（REST + 事件流双通道），git 走 /api/workspace/git-info。
 * 分支图 = 起步形态（提交节点时间线）；完整 DAG 图留 M2 深化轮。
 *
 * 顶部项目横条已于 2026-08-15 退役（用户"选择项目占一整行不合理"）：
 * 项目切换器 + git 状态（分支/变更摘要）并入文件树单元（FilePanel
 * coding 模式）；本壳只负责内容区布局，不再持有任何业务状态。
 *
 * 视图 = 宿主共享公共组件（FilePanel/Editor/TodoPanel/ChatPane/TerminalPane
 * 全部在 lib/dock-views.tsx 登记，零改动嵌入）：对话视图单实例且绑定 coding
 * 场景（一软件一会话，面板挂载即 ensureAppSession），终端 = xterm.js +
 * portable-pty 上游吸收。布局快照持久化 + 导航右键重置 + 空组重开由
 * DockLayout 承担。
 */
import { DockLayout } from "@/components/layout/DockLayout";

export function CodingApp() {
  return (
    <div className="flex h-full min-w-0 flex-col bg-background">
      <div className="min-h-0 flex-1">
        <DockLayout appId="coding" />
      </div>
    </div>
  );
}
