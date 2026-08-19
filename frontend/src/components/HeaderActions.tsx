// dockview 标题栏右侧操作：最大化 + 关闭（macOS 风圆点）。
// 常态只显示低调小点，hover 变图标。所有 dockview 单元共用，样式统一。

// dockview 传真实 IDockviewHeaderActionsProps（activePanel 是 IDockviewPanel，
// close/maximize 在 .api 上）。类型匹配用宽松解构，避免依赖库内部类型。
export default function HeaderActions(props: any) {
  const api = props?.activePanel?.api;
  if (!api?.close || !api?.maximize) return null;
  return (
    <div className="bm-header-actions">
      {/* 最大化：常态圆点，hover 变方块图标 */}
      <button
        className="bm-header-action bm-header-max"
        title="最大化"
        onClick={() => api.maximize?.()}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.3">
          <rect x="1" y="1" width="8" height="8" rx="1" />
        </svg>
      </button>
      {/* 关闭：常态圆点，hover 变 ✕ */}
      <button
        className="bm-header-action bm-header-close"
        title="关闭"
        onClick={() => api.close?.()}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.3">
          <line x1="2" y1="2" x2="8" y2="8" />
          <line x1="8" y1="2" x2="2" y2="8" />
        </svg>
      </button>
    </div>
  );
}
