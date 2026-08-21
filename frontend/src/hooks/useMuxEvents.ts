// 全局 mux 帧总线：聚合「非聊天帧」——approval/requested、approval/resolved、
// session/projection——供审批弹窗、目标卡片等应用级组件订阅。
// 单条 WS 连接（模块级单例），组件挂载即注册监听、卸载即退订；连接断开 2s 重连。
// 聊天实时帧（session/event）仍走 useChat 自有连接，互不干扰。

import { useEffect } from "react";

export interface ApprovalRequested {
  rpcId: string;
  sessionId: string;
  approvalId: string;
  toolName: string;
  callId?: string;
  reason?: string;
}

export interface ApprovalResolved {
  sessionId: string;
  approvalId: string;
  outcome: "allowed-once" | "rejected";
}

export interface ProjectionFrame {
  sessionId: string;
  key: string;
  value: unknown;
  seq: number;
}

export type MuxMethod = "approval/requested" | "approval/resolved" | "session/projection";

export interface MuxFrame {
  method: MuxMethod;
  rpcId: string;
  payload: unknown;
}

const listeners = new Set<(f: MuxFrame) => void>();
let started = false;

function notify(f: MuxFrame) {
  for (const l of listeners) l(f);
}

function ensureStarted() {
  if (started) return;
  started = true;
  const connect = () => {
    const ws = new WebSocket(`ws://${location.host}/api/events.mux`);
    ws.onmessage = (e) => {
      let rec: MuxFrame | null = null;
      try {
        const raw = JSON.parse(e.data as string);
        const method = raw?.method;
        if (
          method === "approval/requested" ||
          method === "approval/resolved" ||
          method === "session/projection"
        ) {
          rec = { method, rpcId: (raw.rpcId as string) ?? "", payload: raw.payload };
        }
      } catch {
        return;
      }
      if (rec) notify(rec);
    };
    ws.onclose = () => {
      setTimeout(connect, 2000);
    };
  };
  connect();
}

/** handler 收到完整帧（含外层 rpcId）——approval 应答需要把帧的 rpcId 原样回显。 */
export function useMuxEvent(method: MuxMethod, handler: (frame: MuxFrame) => void) {
  useEffect(() => {
    ensureStarted();
    const onFrame = (f: MuxFrame) => {
      if (f.method === method) handler(f);
    };
    listeners.add(onFrame);
    return () => {
      listeners.delete(onFrame);
    };
  }, [method, handler]);
}