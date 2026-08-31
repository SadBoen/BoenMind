// W1(ADR-0014):BoenMind WebUI 壳子——三栏布局骨架 + 对话闭环
// 布局蓝本与设计令牌见 milestones/W1-implementation-spec.md §3
import { BoenmindRuntimeProvider } from "./w1/runtime";
import { Thread } from "./w1/thread";

export default function App() {
  return (
    <BoenmindRuntimeProvider>
      <div className="app">
        <Rail />
        <SessionPanel />
        <Thread />
        <WorkspacePanel />
      </div>
    </BoenmindRuntimeProvider>
  );
}

function Rail() {
  return (
    <div className="rail">
      <button className="rail-btn active" title="聊天">💬</button>
      <button className="rail-btn" title="日程(规划中)">📅</button>
      <button className="rail-btn" title="组件(规划中)">🧩</button>
      <button className="rail-btn" title="图层(规划中)">🗂</button>
      <button className="rail-btn" title="浏览(规划中)">🌐</button>
      <button className="rail-btn" title="文件(规划中)">📁</button>
      <button className="rail-btn" title="清单(规划中)">📋</button>
      <div className="rail-spacer" />
      <button className="rail-btn" title="设置(W2 接入)">⚙</button>
    </div>
  );
}

function SessionPanel() {
  return (
    <div className="sessions">
      <div className="sessions-head">
        <span className="title">聊天</span>
        <button className="icon-chip" title="新建对话">＋</button>
      </div>
      <input className="search" placeholder="搜索对话…" disabled />
      <div className="chips">
        <span className="chip active">全部</span>
        <span className="chip">未分配</span>
      </div>
      <div className="session-item">
        <span className="name">BoenMind 对话</span>
        <span className="meta">刚刚</span>
      </div>
      <div className="sessions-empty">会话列表真数据随 W2 接入</div>
    </div>
  );
}

function WorkspacePanel() {
  return (
    <div className="workspace">
      <div className="workspace-head">
        <span className="title">WORKSPACE</span>
        <span className="actions">
          <button className="icon-chip" title="新建(规划中)">＋</button>
          <button className="icon-chip" title="同步(规划中)">⟳</button>
        </span>
      </div>
      <div className="ws-tabs">
        <button className="ws-tab">文件</button>
        <button className="ws-tab">产物</button>
        <button className="ws-tab active">待办</button>
      </div>
      <div className="ws-empty">此会话暂无活动任务列表。</div>
    </div>
  );
}
