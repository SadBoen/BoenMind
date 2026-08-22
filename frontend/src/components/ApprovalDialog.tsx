import { useStore } from "../store";

/** 工具审批卡：后端 approval/requested（危险工具执行前的允许/拒绝门）。
 * 首项弹卡、按序处理；应答走 cmd 命令层 → POST /api/respond。
 * 不处理审批会导致会话永久挂起（agent 在等应答）。 */
export function ApprovalDialog() {
  const { state, dispatch } = useStore();
  const a = state.pendingApprovals[0];
  if (!a) return null;
  const rest = state.pendingApprovals.length - 1;
  return (
    <div className="modal-center" role="dialog" aria-modal="true" aria-labelledby="approval-title">
      <div className="dialog-card">
        <h3 id="approval-title">工具审批请求</h3>
        <p>
          会话 <code>{a.sessionId.slice(0, 8)}</code> 请求执行工具 <b>{a.toolName}</b>
        </p>
        {a.reason && (
          <pre className="think-body" style={{ maxHeight: "180px", overflow: "auto" }}>
            {a.reason}
          </pre>
        )}
        <p style={{ color: "var(--fg-3)" }}>
          允许前请确认请求内容可信。拒绝不会中断会话。
          {rest > 0 && `（其后还有 ${rest} 个待审批）`}
        </p>
        <div className="modal-actions">
          <button
            type="button"
            className="btn-ghost"
            onClick={() => dispatch({ type: "approval-respond", rpcId: a.rpcId, outcome: "rejected" })}
          >
            拒绝
          </button>
          <button
            type="button"
            className="btn-solid"
            onClick={() => dispatch({ type: "approval-respond", rpcId: a.rpcId, outcome: "allowed-once" })}
          >
            允许一次
          </button>
        </div>
      </div>
    </div>
  );
}
