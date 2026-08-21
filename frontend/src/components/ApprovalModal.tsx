// 工具审批弹窗：全局监听 approval/requested 帧 → 弹出确认；用户批准/拒绝 →
// POST /api/respond（回显帧的 rpcId + approvalId + outcome）。后端随帧带 callId 时
// 展示工具参数摘要，便于用户判断。超时由后端兜底（APPROVAL_TIMEOUT=600s → 拒绝）。

import { useState } from "react";
import { Button, Modal, notification } from "antd";
import { ApprovalRequested, ApprovalResolved, MuxFrame, useMuxEvent } from "../hooks/useMuxEvents";
import { rpc } from "../client";

interface PendingApproval extends ApprovalRequested {
  ts: number;
}

export default function ApprovalModal() {
  const [pending, setPending] = useState<PendingApproval[]>([]);
  const [busy, setBusy] = useState(false);

  // approval/resolved 到达：按 approvalId 移除对应弹窗（防止重复应答 / 响应必达）。
  useMuxEvent("approval/resolved", (f: MuxFrame) => {
    const p = f.payload as ApprovalResolved;
    if (!p?.approvalId) return;
    setPending((list) => list.filter((a) => a.approvalId !== p.approvalId));
  });

  // 收集帧。mux 流里本项目工具审批也广播给所有连接 —— 这里按 approvalId 去重。
  // 帧的应答 key = 外层 rpcId（respond 路由用），approvalId 是展示/校验 id。
  useMuxEvent("approval/requested", (f: MuxFrame) => {
    const p = f.payload as ApprovalRequested;
    if (!p?.approvalId) return;
    setPending((list) => {
      if (list.some((a) => a.approvalId === p.approvalId)) return list;
      const next = [...list, { ...p, rpcId: f.rpcId, ts: Date.now() }];
      return next.sort((x, y) => x.ts - y.ts);
    });
  });

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
      // 后端会广播 approval/resolved → 上面 handler 移除弹窗。若已超时移除（后端 600s 超时拒绝），
      // 这里也兜底移除，用户不会看到僵死弹窗。
      setPending((list) => list.filter((a) => a.approvalId !== item.approvalId));
    } catch {
      notification.error({ message: "审批应答失败", description: "网络错误，请重试", placement: "bottomRight" });
    } finally {
      setBusy(false);
    }
  };

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
          <div className="approval-tool">{current.toolName}</div>
          {current.callId && <div className="approval-call">调用 {current.callId}</div>}
          {current.reason && <div className="approval-reason">{current.reason}</div>}
          {pending.length > 1 && (
            <div className="approval-queue">另有 {pending.length - 1} 个待审批</div>
          )}
          <div className="approval-actions">
            <Button onClick={() => respond(current, "rejected")} danger disabled={busy}>
              拒绝
            </Button>
            <Button
              type="primary"
              onClick={() => respond(current, "allowed-once")}
              loading={busy}
            >
              仅本次允许
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
}