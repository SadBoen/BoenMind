// 工具审批弹窗：全局监听 approval/requested 帧 → 弹出确认；用户批准/拒绝 →
// POST /api/respond（回显帧的 rpcId + approvalId + outcome）。后端随帧带 callId 时
// 展示工具参数摘要，便于用户判断。超时由后端兜底（APPROVAL_TIMEOUT=600s → 拒绝）。

import { useRef, useState } from "react";
import { Button, Modal, notification } from "antd";
import { ApprovalRequested, ApprovalResolved, MuxFrame, useMuxEvent } from "../hooks/useMuxEvents";

interface PendingApproval extends ApprovalRequested {
  ts: number;
}

// 危险工具名单（对齐后端各插件 DANGEROUS_TOOL_NAMES 汇合；仅展示层标识用）。
const DANGEROUS_TOOLS = new Set([
  "host.run_command",
  "code.compile",
  "code.python",
  "code.shell",
  "web.fetch",
  "goal.create",
  "goal.update",
  "schedule.create",
]);

export default function ApprovalModal() {
  const [pending, setPending] = useState<PendingApproval[]>([]);
  const [busy, setBusy] = useState(false);
  // 会话级豁免：「本会话信任此工具」后，同 sessionId + toolName 的后续请求自动放行
  // （allowed-once 语义；纯前端豁免层不消耗后端契约）。用 ref 承载豁免表，
  // 事件 handler 总是读到最新值（不受渲染闭包时序影响）。
  const trustedRef = useRef<Record<string, string[]>>({});
  const [trustedKeys, setTrustedKeys] = useState<Record<string, string[]>>({}); // 仅驱动按钮态/展示
  const [trustBusy, setTrustBusy] = useState(false);

  const isTrusted = (sessionId: string, toolName: string) =>
    (trustedRef.current[sessionId] ?? []).includes(toolName);

  // 核心应答：POST /api/respond（回显帧 rpcId + approvalId + outcome）。
  const respond = async (item: PendingApproval, outcome: "allowed-once" | "rejected") => {
    setBusy(true);
    try {
      const body = {
        type: "client-response",
        rpcId: item.rpcId,
        result: { ok: true, value: { sessionId: item.sessionId, approvalId: item.approvalId, outcome } },
      };
      const res = await fetch("/api/respond", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const receipt = (await res.json().catch(() => null)) as { accepted?: boolean; reason?: string } | null;
      if (receipt?.accepted !== true) {
        notification.warning({ message: "审批应答未送达", description: receipt?.reason ?? "未知原因", placement: "bottomRight" });
        return;
      }
      if (outcome === "rejected") {
        notification.info({ message: "已拒绝该工具调用", placement: "bottomRight" });
      }
      // 后端会广播 approval/resolved → handler 移除弹窗。若已超时移除（后端 600s 超时拒绝），
      // 这里也兜底移除，用户不会看到僵死弹窗。
      setPending((list) => list.filter((a) => a.approvalId !== item.approvalId));
    } catch {
      notification.error({ message: "审批应答失败", description: "网络错误，请重试", placement: "bottomRight" });
    } finally {
      setBusy(false);
      setTrustBusy(false);
    }
  };

  // 「本会话信任该工具」：允许本次 + 把 (sessionId, toolName) 记入豁免表（后续自动放行）。
  const trustTool = async (item: PendingApproval) => {
    setTrustBusy(true);
    trustedRef.current = {
      ...trustedRef.current,
      [item.sessionId]: [...(trustedRef.current[item.sessionId] ?? []), item.toolName],
    };
    setTrustedKeys(trustedRef.current);
    await respond(item, "allowed-once");
  };

  // approval/requested 帧到达：先查豁免 → 命中自动放行（allowed-once，不打扰）；否则入弹窗队列。
  useMuxEvent("approval/requested", (f: MuxFrame) => {
    const p = f.payload as ApprovalRequested;
    if (!p?.approvalId || !p.sessionId || !p.toolName) return;
    if (isTrusted(p.sessionId, p.toolName)) {
      void respond({ ...p, rpcId: f.rpcId, ts: Date.now() }, "allowed-once");
      return;
    }
    setPending((list) => {
      if (list.some((a) => a.approvalId === p.approvalId)) return list;
      const next = [...list, { ...p, rpcId: f.rpcId, ts: Date.now() }];
      return next.sort((x, y) => x.ts - y.ts);
    });
  });

  const current = pending[0];
  return (
    <Modal
      open={pending.length > 0}
      closable={false}
      maskClosable={false}
      keyboard={false}
      width={420}
      footer={null}
      title="工具调用审批"
      className="approval-modal"
    >
      {current && (
        <div className="approval-body">
          <div className="approval-tool">
            {current.toolName}
            {DANGEROUS_TOOLS.has(current.toolName) && (
              <span className="approval-danger-badge">危险</span>
            )}
          </div>
          {current.callId && <div className="approval-call">调用 {current.callId}</div>}
          {current.reason && <div className="approval-reason">{current.reason}</div>}
          {pending.length > 1 && (
            <div className="approval-queue">另有 {pending.length - 1} 个待审批</div>
          )}
          {(trustedKeys[current.sessionId] ?? []).length > 0 && (
            <div className="approval-trusted">
              本会话已信任 {(trustedKeys[current.sessionId] ?? []).length} 个工具（同名调用自动放行）
            </div>
          )}
          <div className="approval-actions">
            <Button onClick={() => respond(current, "rejected")} danger disabled={busy || trustBusy}>
              拒绝
            </Button>
            <Button
              onClick={() => trustTool(current)}
              loading={trustBusy}
              disabled={busy}
            >
              本会话信任该工具
            </Button>
            <Button
              type="primary"
              onClick={() => respond(current, "allowed-once")}
              loading={busy}
              disabled={trustBusy}
            >
              仅本次允许
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
}