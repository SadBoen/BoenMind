/**
 * 分支图泳道分配（纯函数，独立文件供组件引用与测试——Fast refresh 纪律：
 * 组件文件不导出非组件符号）。
 *
 * 输入 commits 必须为拓扑序（新 → 旧，git log 输出形态）。
 * 算法：从旧到新遍历，提交取第一个图中父提交的 lane；无图中父（链端）
 * 取空闲 lane。lane 在"该 lane 上最新端提交（图中无子提交）处理完"后
 * 释放——被释放后可供其他链复用。正确性优先，非最优 lane 数。
 */
import type { GitInfo } from "@/api/client";

export interface CommitRow {
  hash: string;
  subject: string;
  parents: string[];
  lane: number;
  isMerge: boolean;
}

export function computeLanes(commits: GitInfo["commits"]): CommitRow[] {
  const list = commits ?? [];
  const idx = new Map(list.map((c, i) => [c.hash, i]));
  // 子提交索引：用于判断 lane 何时可释放（该 lane 最新端处理完）
  const children = new Map<string, string[]>();
  for (const c of list) {
    for (const p of c.parents) {
      if (!idx.has(p)) continue;
      children.set(p, [...(children.get(p) ?? []), c.hash]);
    }
  }
  const laneOf = new Map<string, number>();
  const busy = new Set<number>();
  let maxLane = -1;
  const freeLane = () => {
    for (let l = 0; l <= maxLane; l++) {
      if (!busy.has(l)) return l;
    }
    maxLane += 1;
    return maxLane;
  };
  // 旧 → 新遍历：父提交（更旧）的 lane 必已分配
  for (let i = list.length - 1; i >= 0; i--) {
    const c = list[i];
    const parentsIn = c.parents.filter((p) => idx.has(p));
    const lane = parentsIn.length > 0 ? laneOf.get(parentsIn[0])! : freeLane();
    laneOf.set(c.hash, lane);
    busy.add(lane);
    if ((children.get(c.hash) ?? []).length === 0) busy.delete(lane);
  }
  return list.map((c) => ({
    hash: c.hash,
    subject: c.subject,
    parents: c.parents,
    lane: laneOf.get(c.hash)!,
    isMerge: c.parents.length > 1,
  }));
}
