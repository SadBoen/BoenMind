// 目标卡片：消费 session/projection 的 goal 投影（WS 增量 + session.history 快照）。
// 展示目标（objective）、阶段徽章（active/paused/blocked/complete）、轮次进度
// （roundsStarted/maxGoalRounds），并提供 pause/resume/complete 操作（goal RPC，CAS 用投影里的 ref）。
// goal 投影 key="goal"，value 形状：
//   { goal:{ id, revision, objective, phase, maxGoalRounds }, roundsStarted, createdAt, updatedAt }
// goal.clear 后投影为 null（墓碑，前端回到无目标空态）。

import { useCallback, useEffect, useRef, useState } from "react";
import { Button, Popconfirm, Tag } from "antd";
import { rpc } from "../client";
import { MuxFrame, ProjectionFrame, useMuxEvent } from "../hooks/useMuxEvents";

export interface GoalProjection {
  goal: {
    id: string;
    revision: number;
    objective: string;
    phase: "active" | "paused" | "blocked" | "complete";
    maxGoalRounds: number;
  };
  roundsStarted: number;
  createdAt: number | null;
  updatedAt: number | null;
}

const PHASE_TAG: Record<string, { color: string; label: string }> = {
  active: { color: "green", label: "进行中" },
  paused: { color: "default", label: "已暂停" },
  blocked: { color: "orange", label: "受阻" },
  complete: { color: "blue", label: "已完成" },
};

export default function GoalCard({ sessionId }: { sessionId: string }) {
  const [goal, setGoal] = useState<GoalProjection | null>(null);
  const [loading, setLoading] = useState(false);
  const seqRef = useRef<number>(-1);
  const [revision, setRevision] = useState<number>(1);

  // 切换会话：拉 history 快照（含投影）重置本地状态。
  useEffect(() => {
    setGoal(null);
    seqRef.current = -1;
    setRevision(1);
    if (!sessionId) return;
    rpc<{ projections: { values?: Record<string, unknown> } }>("session.history", { sessionId })
      .then((h) => {
        const values = h.projections?.values ?? {};
        const v = values["goal"] as GoalProjection | null | undefined;
        if (v) {
          setGoal(v);
          seqRef.current = 0; // 快照无 seq（asOfSeq 指事件），增量 seq 从 1+ 生效
          setRevision(v.goal.revision);
        }
      })
      .catch(() => {});
  }, [sessionId]);

  // 增量:session/projection 帧按 higher-seq-wins 合并（seq 单调递增，key="goal"）。
  const onProjection = useCallback(
    (f: MuxFrame) => {
      const payload = f.payload as ProjectionFrame;
      if (!payload || payload.key !== "goal") return;
      if (!sessionId || payload.sessionId !== sessionId) return;
      if (payload.seq <= seqRef.current) return;
      seqRef.current = payload.seq;
      if (payload.value === null) {
        setGoal(null);
        setRevision(1);
        return;
      }
      setGoal(payload.value as GoalProjection);
      setRevision((payload.value as GoalProjection).goal.revision);
    },
    [sessionId]
  );
  useMuxEvent("session/projection", onProjection);

  // goal RPC（CAS：ref:{id,revision}）。成功由投影广播回灌；失败展示原因。
  const change = async (method: string, okMsg: string) => {
    if (!goal) return;
    setLoading(true);
    try {
      await rpc(method, { sessionId, ref: { id: goal.goal.id, revision: revision } });
    } catch (e) {
      // notification 在卡片容错下静默（投影会纠正）；错误大多为 goal-conflict（并发续跑已改 rev）
    } finally {
      setLoading(false);
    }
  };

  if (!goal) return null;

  const tag = PHASE_TAG[goal.goal.phase] ?? { color: "default", label: goal.goal.phase };
  const rounds = goal.roundsStarted;
  const max = goal.goal.maxGoalRounds;
  const pct = Math.min(100, max > 0 ? Math.round((rounds / max) * 100) : 0);

  return (
    <div className="goal-card">
      <div className="goal-card-head">
        <span className="goal-card-title">🎯 目标</span>
        <Tag color={tag.color} className="goal-card-tag">{tag.label}</Tag>
        {goal.goal.phase === "active" && (
          <span className="goal-card-rounds">第 {rounds}/{max} 轮</span>
        )}
      </div>
      <div className="goal-card-objective">{goal.goal.objective}</div>
      {goal.goal.phase === "active" && (
        <div className="goal-card-progress">
          <div className="goal-card-bar">
            <div className="goal-card-bar-fill" style={{ width: `${pct}%` }} />
          </div>
        </div>
      )}
      <div className="goal-card-actions">
        {goal.goal.phase === "active" && (
          <Button size="small" loading={loading} onClick={() => change("goal.pause", "已暂停")}>暂停</Button>
        )}
        {goal.goal.phase === "paused" && (
          <Button size="small" loading={loading} onClick={() => change("goal.resume", "已恢复")}>恢复</Button>
        )}
        {goal.goal.phase !== "complete" && goal.goal.phase !== "blocked" && (
          <Popconfirm title="完成并解除此目标？" onConfirm={() => change("goal.complete", "已完成")}>
            <Button size="small" danger loading={loading}>完成</Button>
          </Popconfirm>
        )}
        {goal.goal.phase === "blocked" && (
          <span className="goal-card-blocked">受阻（等待处理）</span>
        )}
      </div>
    </div>
  );
}