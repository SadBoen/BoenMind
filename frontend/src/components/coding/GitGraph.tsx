/**
 * 分支图 DAG 视图（M2 深化）：git 提交拓扑的可视化（merge/分叉泳道）。
 *
 * 数据：/api/workspace/git-info（commits 含 parents 拓扑边 + 本地分支指针）。
 * 渲染：SVG 泳道图——每提交一行，lane 分配（lib/git-lanes.ts）保证同一主链
 * 同列、merge/分叉跨列连线；提交节点圆点（merge 提交空心），分支标签锚在
 * tip 提交行（当前分支高亮）。
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { api, type GitInfo } from "@/api/client";
import { computeLanes } from "@/lib/git-lanes";
import { useAppStore } from "@/stores/app-store";

const ROW_H = 26;
const LANE_W = 18;
const PAD_LEFT = 8;
const NODE_R = 4;

export function GitGraph() {
  const { t } = useTranslation();
  const [git, setGit] = useState<GitInfo | null>(null);
  // 项目根（项目切换：分支图跟随当前项目）
  const projectRoot = useAppStore((s) => s.currentProject?.root);
  const load = useCallback(() => {
    api
      .gitInfo(projectRoot)
      .then(setGit)
      .catch(() => setGit(null));
  }, [projectRoot]);
  useEffect(() => {
    load();
  }, [load]);

  if (!git?.repo) {
    return (
      <div className="flex h-full items-center justify-center bg-background text-sm text-muted-foreground">
        {t("coding.git.noRepo")}
      </div>
    );
  }

  const rows = computeLanes(git.commits);
  const laneCount = rows.reduce((m, r) => Math.max(m, r.lane + 1), 1);
  const width = PAD_LEFT + laneCount * LANE_W + 280;
  const height = Math.max(rows.length, 1) * ROW_H + 6;
  const idx = new Map(rows.map((r, i) => [r.hash, i]));
  const xOf = (lane: number) => PAD_LEFT + lane * LANE_W + LANE_W / 2;
  const yOf = (i: number) => i * ROW_H + ROW_H / 2;
  const branches = git.branches ?? [];

  return (
    <div className="relative h-full overflow-auto bg-background">
      <Button
        variant="ghost"
        size="icon"
        className="absolute right-2 top-2 z-10 h-7 w-7"
        title={t("common.refresh")}
        onClick={load}
      >
        <RefreshCw size={13} />
      </Button>
      <svg width={width} height={height} className="text-muted-foreground">
        {/* 泳道背景线 */}
        {Array.from({ length: laneCount }, (_, l) => (
          <line
            key={l}
            x1={xOf(l)}
            y1={4}
            x2={xOf(l)}
            y2={height - 2}
            className="stroke-border"
            strokeWidth={1}
          />
        ))}
        {/* 跨泳道边（同泳道的主链线由背景线承担）：merge 汇入 + 分叉引出 */}
        {rows.flatMap((r, i) =>
          r.parents
            .filter((p) => idx.has(p))
            .filter((p) => rows[idx.get(p)!].lane !== r.lane)
            .map((p, k) => {
              const pRow = rows[idx.get(p)!];
              const x1 = xOf(pRow.lane);
              const y1 = yOf(idx.get(p)!);
              const x2 = xOf(r.lane);
              const y2 = yOf(i);
              const mid = (y1 + y2) / 2;
              return (
                <path
                  key={`${r.hash}-${k}`}
                  d={`M${x1} ${y1} C ${x1} ${mid}, ${x2} ${mid}, ${x2} ${y2}`}
                  fill="none"
                  strokeWidth={1.5}
                  className="stroke-muted-foreground/40"
                />
              );
            }),
        )}
        {/* 提交节点 + 摘要 */}
        {rows.map((r, i) => {
          const cx = xOf(r.lane);
          const cy = yOf(i);
          return (
            <g key={r.hash}>
              <circle
                cx={cx}
                cy={cy}
                r={NODE_R}
                fill={r.isMerge ? "none" : "currentColor"}
                strokeWidth={1.5}
                className={r.isMerge ? "stroke-primary" : "fill-primary stroke-primary"}
              />
              <text x={cx + 9} y={cy + 4} fontSize={12} className="fill-muted-foreground">
                {r.subject}
              </text>
            </g>
          );
        })}
        {/* 分支标签：锚在 tip 提交行 */}
        {branches.flatMap((b) => {
          const i = idx.get(b.tip);
          if (i === undefined) return [];
          const current = b.name === git.branch;
          const cx = xOf(rows[i].lane);
          return (
            <g key={b.name}>
              <rect
                x={cx + 8}
                y={yOf(i) - 9}
                rx={7}
                height={14}
                width={b.name.length * 10 + 14}
                className={current ? "fill-primary/15 stroke-primary/40" : "fill-muted stroke-border"}
                strokeWidth={1}
              />
              <text
                x={cx + 15}
                y={yOf(i) + 1}
                fontSize={9}
                className={current ? "fill-primary" : "fill-muted-foreground"}
              >
                {b.name}
              </text>
            </g>
          );
        })}
      </svg>
    </div>
  );
}
