// W10(ADR-0024)「限制与超时」设置页:运行时全部限制项集中可视可改。
// 数据源 = GET/PUT /admin/limits(服务端统一钳制兜底,缺文件=默认);
// 保存即热生效(命令/工具下一条、回合下一回合、流式下一条起)。
import { useCallback, useEffect, useMemo, useState } from "react";
import { RotateCcwIcon, SaveIcon, ShieldAlertIcon } from "lucide-react";
import { api, type LimitKey } from "./api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { useTimedNotice } from "@/lib/use-timed-notice";

// 数值展示口径:毫秒/字节大数按原值展示,输入框走字符串态由用户编辑
type Draft = Record<string, string>;

function fmtNum(n: number): string {
  return String(n);
}

function humanRange(min: number, max: number): string {
  return `${min} ~ ${max}`;
}

export function LimitsPage() {
  const [keys, setKeys] = useState<LimitKey[] | null>(null);
  const [draft, setDraft] = useState<Draft>({});
  const { notice, flash } = useTimedNotice(4000);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    try {
      const res = await api.limits.get();
      setKeys(res.keys);
      const d: Draft = {};
      for (const k of res.keys) d[k.key] = fmtNum(k.value);
      setDraft(d);
    } catch (e) {
      flash(`加载失败:${e instanceof Error ? e.message : String(e)}`);
    }
  }, [flash]);

  useEffect(() => {
    void load();
  }, [load]);

  const groups = useMemo(() => {
    const g = new Map<string, LimitKey[]>();
    for (const k of keys ?? []) {
      const list = g.get(k.group) ?? [];
      list.push(k);
      g.set(k.group, list);
    }
    return [...g.entries()];
  }, [keys]);

  const dirty = useMemo(() => {
    if (!keys) return false;
    return keys.some((k) => Number(draft[k.key]) !== k.value);
  }, [keys, draft]);

  const save = async () => {
    if (!keys) return;
    const values: Record<string, number> = {};
    for (const k of keys) {
      const n = Number(draft[k.key]);
      if (!Number.isFinite(n)) {
        flash(`「${k.label}」不是有效数字,未保存`);
        return;
      }
      values[k.key] = n;
    }
    setSaving(true);
    try {
      const res = await api.limits.put(values);
      flash(res.note || "已保存");
      await load();
    } catch (e) {
      flash(`保存失败:${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSaving(false);
    }
  };

  const resetAll = async () => {
    // 全量恢复出厂:PUT 空 values = 服务端以默认值落盘
    setSaving(true);
    try {
      const res = await api.limits.put({});
      flash(res.note || "已恢复默认");
      await load();
    } catch (e) {
      flash(`恢复失败:${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSaving(false);
    }
  };

  if (!keys) {
    return <div className="text-muted-foreground text-[13px]">加载中…</div>;
  }

  return (
    <div data-slot="limits-page">
      <div className="mb-4 flex items-center gap-2">
        <span className="text-[15px] font-semibold">限制与超时</span>
        <span className="text-muted-foreground text-[12px]">
          保存即热生效;误改可单键或全局恢复默认
        </span>
        <span className="flex-1" />
        <Button variant="outline" size="sm" onClick={() => void resetAll()} disabled={saving}>
          <RotateCcwIcon className="mr-1 size-3.5" />
          全部恢复默认
        </Button>
        <Button size="sm" onClick={() => void save()} disabled={saving || !dirty}>
          <SaveIcon className="mr-1 size-3.5" />
          {saving ? "保存中…" : "保存"}
        </Button>
      </div>

      {notice ? (
        <div
          className="bg-card mb-3 rounded-lg border px-3 py-2 text-[13px]"
          data-slot="limits-notice"
        >
          {notice}
        </div>
      ) : null}

      {groups.map(([group, items]) => (
        <section key={group} className="mb-5" data-slot="limits-group" data-group={group}>
          <div className="text-muted-foreground mb-2 text-[12px] font-medium tracking-wide">
            {group}
          </div>
          <div className="bg-card overflow-hidden rounded-xl border">
            {items.map((k, i) => {
              const changed = Number(draft[k.key]) !== k.value;
              return (
                <div
                  key={k.key}
                  className={cn(
                    "flex items-center gap-3 px-3 py-2",
                    i > 0 && "border-t",
                  )}
                  data-slot="limits-row"
                  data-key={k.key}
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="text-[13px] font-medium">{k.label}</span>
                      {k.source === "env" ? (
                        <Badge variant="outline" className="text-[10px]">
                          环境变量覆盖中
                        </Badge>
                      ) : null}
                      {changed ? (
                        <Badge className="text-[10px]">未保存</Badge>
                      ) : null}
                    </div>
                    <div className="text-muted-foreground font-mono text-[11px]">
                      {k.key} · 允许范围 {humanRange(k.min, k.max)}
                      {k.source === "env"
                        ? " · 此项被启动环境变量覆盖,改文件需去除环境变量后重启"
                        : ""}
                    </div>
                  </div>
                  <Input
                    className="h-7 w-36 text-right font-mono text-[12px]"
                    value={draft[k.key] ?? ""}
                    inputMode="numeric"
                    disabled={!k.editable}
                    onChange={(e) =>
                      setDraft((prev) => ({ ...prev, [k.key]: e.target.value }))
                    }
                    data-slot="limits-input"
                  />
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2"
                    title="恢复该项默认值"
                    disabled={!k.editable || Number(draft[k.key]) === k.default}
                    onClick={() =>
                      setDraft((prev) => ({ ...prev, [k.key]: fmtNum(k.default) }))
                    }
                  >
                    <RotateCcwIcon className="size-3.5" />
                  </Button>
                </div>
              );
            })}
          </div>
        </section>
      ))}

      <div className="text-muted-foreground flex items-start gap-1.5 text-[11px]">
        <ShieldAlertIcon className="mt-0.5 size-3.5 shrink-0" />
        <span>
          所有值保存到数据目录 config/limits.json,服务端会自动夹到允许范围内;
          个别编译期常量(如前端 60 秒静默看门狗)不在此列。
        </span>
      </div>
    </div>
  );
}
