// W10(ADR-0025):后台作业徽标——吸顶可见的「在跑后台命令」计数。
// 10s 轮询 /admin/jobs;无作业不渲染(零打扰);悬停提示各作业命令。
import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { api } from "@/w2/api";

export function JobsBadge() {
  const [jobs, setJobs] = useState<
    { id: string; command: string; status: string; elapsed_ms: number }[]
  >([]);

  useEffect(() => {
    let alive = true;
    const poll = async () => {
      try {
        const res = await api.jobs();
        if (alive) setJobs(res.jobs ?? []);
      } catch {
        // 管理面不可达(未挂载/门户态)静默——徽标本就是锦上添花
      }
    };
    void poll();
    const t = window.setInterval(() => void poll(), 10_000);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, []);

  const running = jobs.filter((j) => j.status === "running");
  if (running.length === 0) return null;

  const tip = running
    .map((j) => `#${j.id}(${Math.round(j.elapsed_ms / 1000)}s):${j.command}`)
    .join("\n");

  return (
    <span
      className="bg-card flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px] text-muted-foreground"
      data-slot="jobs-badge"
      title={`后台作业执行中(不受前台超时限制):\n${tip}`}
    >
      <Loader2 className="size-3 animate-spin text-amber-500" />
      后台 {running.length}
    </span>
  );
}
