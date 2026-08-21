// 目标卡片：消费 session/projection 的 goal 投影（WS 增量 + session.history 快照）。
// 展示目标（objective）、阶段徽章（active/paused/blocked/complete）、轮次进度
// （roundsStarted/maxGoalRounds），并提供 pause/resume/complete 操作（goal RPC，CAS 用投影里的 ref）。
// goal 投影 key="goal"，value 形状：
//   { goal:{ id, revision, objective, phase, maxGoalRounds }, roundsStarted, createdAt, updatedAt }
// goal.clear 后投影为 null（墓碑，前端回到无目标空态）。

import { useCallback, useEffect, useRef, useState } from "react";
import { Button, Input, Popconfirm, Space, Tag } from "antd";
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
  // 新建目标表单态（无目标时显示）
  const [creating, setCreating] = useState(false);
  const [objective, setObjective] = useState("");
  const [maxRounds, setMaxRounds] = useState(8);
  const [createBusy, setCreateBusy] = useState(false);
  // 编辑已有目标表单态（展示态点击「编辑」进入）
  const [editing, setEditing] = useState(false);
  const [editObjective, setEditObjective] = useState("");
  const [editRounds, setEditRounds] = useState(8);
  const [editBusy, setEditBusy] = useState(false);

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
      .catch(() => {})
      .finally(() => setEditing(false));
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
        // goal.clear 墓碑：退出编辑态，回到无目标空态。
        setGoal(null);
        setRevision(1);
        setEditing(false);
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

  // 新建目标：goal.create → 成功后靠投影广播回灌切展示态（本会话立即有投影）。
  const create = async () => {
    if (!objective.trim() || createBusy) return;
    setCreateBusy(true);
    try {
      await rpc("goal.create", {
        sessionId,
        objective: objective.trim(),
        maxGoalRounds: Math.max(1, Math.round(maxRounds)),
      });
      setObjective("");
      setCreating(false);
    } catch (e) {
      // 失败保留表单（用户可改）；错误 message 由 rpc 抛，这里静默避免打断
    } finally {
      setCreateBusy(false);
    }
  };

  // 编辑已有目标：goal.edit（CAS ref 同 change）。至少改一项；成功靠投影回灌，失败保留表单。
  const edit = async () => {
    if (!goal || editBusy) return;
    // 过滤无效编辑：objective 为空或与现值等、轮次相同 → 视为未修改，直接退出。
    const obj = editObjective.trim();
    const changed =
      (obj !== "" && obj !== goal.goal.objective) ||
      editRounds !== goal.goal.maxGoalRounds;
    if (!changed) {
      setEditing(false);
      return;
    }
    setEditBusy(true);
    try {
      await rpc("goal.edit", {
        sessionId,
        ref: { id: goal.goal.id, revision: revision },
        ...(obj !== "" && obj !== goal.goal.objective ? { objective: obj } : {}),
        ...(editRounds !== goal.goal.maxGoalRounds
          ? { maxGoalRounds: Math.max(1, Math.round(editRounds)) }
          : {}),
      });
      setEditing(false);
    } catch (e) {
      // 冲突/校验失败：保留表单（投影会纠正显示），可再改后再提交
    } finally {
      setEditBusy(false);
    }
  };

  // 进入编辑态：用当前目标预填表单。
  const beginEdit = () => {
    if (!goal) return;
    setEditObjective(goal.goal.objective);
    setEditRounds(goal.goal.maxGoalRounds);
    setEditing(true);
  };

  if (!goal) {
    // 无目标空态：一行「🎯 新建目标」入口 → 展开创建表单。
    return (
      <div className="goal-card goal-card-empty">
        {!creating ? (
          <button className="goal-card-create-btn" type="button" onClick={() => setCreating(true)}>
            🎯 新建目标
          </button>
        ) : (
          <div className="goal-card-create">
            <div className="goal-card-head">
              <span className="goal-card-title">🎯 新建目标</span>
            </div>
            <Input.TextArea
              className="goal-create-input"
              rows={2}
              value={objective}
              placeholder="要达成的目标，例如：完成 5 个 Rust 所有权规则的讲解并验证"
              autoFocus
              onChange={(e) => setObjective(e.target.value)}
            />
            <div className="goal-create-row">
              <span className="goal-create-label">自动续跑轮次</span>
              <Input
                className="goal-create-rounds"
                type="number"
                min={1}
                max={64}
                value={maxRounds}
                onChange={(e) => setMaxRounds(Number(e.target.value) || 1)}
              />
              <span className="goal-create-hint">回合完成后自动续跑，直到目标完成或额度耗尽</span>
            </div>
            <div className="goal-card-actions">
              <Button size="small" onClick={() => setCreating(false)}>取消</Button>
              <Button
                size="small"
                type="primary"
                loading={createBusy}
                disabled={!objective.trim()}
                onClick={create}
              >
                创建目标
              </Button>
            </div>
          </div>
        )}
      </div>
    );
  }

  const tag = PHASE_TAG[goal.goal.phase] ?? { color: "default", label: goal.goal.phase };
  const rounds = goal.roundsStarted;
  const max = goal.goal.maxGoalRounds;
  const pct = Math.min(100, max > 0 ? Math.round((rounds / max) * 100) : 0);

  if (editing) {
    // 编辑态：可改 objective / 自动续跑轮次，保存走 goal.edit。
    return (
      <div className="goal-card goal-card-editing">
        <div className="goal-card-head">
          <span className="goal-card-title">🎯 编辑目标</span>
          <Tag color={tag.color} className="goal-card-tag">{tag.label}</Tag>
        </div>
        <Input.TextArea
          className="goal-create-input"
          rows={2}
          value={editObjective}
          placeholder="目标描述"
          onChange={(e) => setEditObjective(e.target.value)}
        />
        <div className="goal-create-row">
          <span className="goal-create-label">自动续跑轮次</span>
          <Input
            className="goal-create-rounds"
            type="number"
            min={1}
            max={64}
            value={editRounds}
            onChange={(e) => setEditRounds(Number(e.target.value) || 1)}
          />
          <span className="goal-create-hint">回合完成后自动续跑，直到目标完成或额度耗尽</span>
        </div>
        <div className="goal-card-actions">
          <Button size="small" disabled={editBusy} onClick={() => setEditing(false)}>取消</Button>
          <Button type="primary" size="small" loading={editBusy} onClick={edit}>保存</Button>
          <span className="goal-card-blocked">未修改任何内容时可直接取消</span>
        </div>
      </div>
    );
  }

  return (
    <div className="goal-card">
      <div className="goal-card-head">
        <span className="goal-card-title">🎯 目标</span>
        <Tag color={tag.color} className="goal-card-tag">{tag.label}</Tag>
        {goal.goal.phase === "active" && (
          <span className="goal-card-rounds">第 {rounds}/{max} 轮</span>
        )}
        <div className="goal-card-ops">
          <Button
            size="small"
            type="text"
            onClick={beginEdit}
            disabled={goal.goal.phase === "complete" || goal.goal.phase === "blocked"}
          >
            编辑
          </Button>
        </div>
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