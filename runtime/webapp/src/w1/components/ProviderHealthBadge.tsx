// issue #12(接口 97727ab 已立):Provider 熔断健康徽标——吸顶可见的「模型网关坏死」提示。
// 10s 轮询 /admin/providers/health;全员健康不渲染(零打扰,同 JobsBadge 惯例);
// 悬停提示各不健康网关的连败/冷却细节。数据只含发生过调用失败的 provider,
// 「提前发现」= 本徽标看熔断态 + 设置页按需「连通测试」主动探针。
import { useEffect, useState } from "react";
import { AlertTriangle } from "lucide-react";
import { api } from "@/w2/api";

type HealthEntry = {
  provider: string;
  status: string;
  fail_streak: number;
  cooldown_until: string | null;
};

export function ProviderHealthBadge() {
  const [health, setHealth] = useState<HealthEntry[]>([]);

  useEffect(() => {
    let alive = true;
    const poll = async () => {
      try {
        const res = await api.providers.health();
        if (alive) setHealth(res.health ?? []);
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

  const bad = health.filter((h) => h.status !== "healthy");
  if (bad.length === 0) return null;

  const tip = bad
    .map(
      (h) =>
        `${h.provider}: ${h.status}(连败 ${h.fail_streak}${
          h.cooldown_until ? `,冷却至 ${h.cooldown_until}` : ""
        })`,
    )
    .join("\n");

  return (
    <span
      className="flex items-center gap-1 rounded-full border border-destructive/40 bg-destructive/10 px-2 py-0.5 text-[11px] text-destructive"
      data-slot="provider-health-badge"
      title={`模型网关熔断(连续失败开闸,冷却后半开探测):\n${tip}`}
    >
      <AlertTriangle className="size-3" />
      网关 {bad.length}
    </span>
  );
}
