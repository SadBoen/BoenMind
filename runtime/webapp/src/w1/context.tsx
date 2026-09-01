// W5 上下文透视:对话区「上下文」页——每次模型调用发了什么、token 用量
// 多少。数据 = /admin/context(context-log.jsonl 尾部,最旧在前);组成占比
// 为前端估算(chars/3,仅量级感知;服务端不做估算),tokens_in/out 为
// provider 真实回报。只看当前会话 = localStorage bm_session 过滤。
import { useCallback, useEffect, useMemo, useState } from "react";
import { RefreshCwIcon, Loader2Icon } from "lucide-react";
import { api, type CtxStep } from "../w2/api";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";

const CATS = {
  system: { label: "系统提示词", color: "#6366f1" },
  tools: { label: "工具定义", color: "#f59e0b" },
  user: { label: "用户消息", color: "#10b981" },
  assistant: { label: "助手消息", color: "#3b82f6" },
  toolres: { label: "工具结果", color: "#14b8a6" },
} as const;
type CatKey = keyof typeof CATS;
const CAT_KEYS = Object.keys(CATS) as CatKey[];

const ROLE_LABEL: Record<string, string> = {
  system: "系统提示词",
  user: "用户消息",
  assistant: "助手消息",
  tool: "工具结果",
};

// 估算:chars/3(中英混合的粗密度;仅用于组成占比,真实值看 tokens_in/out)
const est = (s: string) => Math.max(1, Math.ceil((s?.length ?? 0) / 3));
// 耗时展示:≥1s 显秒,否则显毫秒
const fmtDur = (ms?: number | null) =>
  ms == null ? "—" : ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;

function catSizes(s: CtxStep): Record<CatKey, number> {
  const c: Record<CatKey, number> = { system: 0, tools: 0, user: 0, assistant: 0, toolres: 0 };
  for (const m of s.messages ?? []) {
    const k: CatKey =
      m.role === "system" ? "system"
      : m.role === "user" ? "user"
      : m.role === "assistant" ? "assistant"
      : "toolres";
    c[k] += est(m.content);
  }
  c.tools = (s.tools ?? []).reduce((n, t) => n + est(JSON.stringify(t ?? {})), 0);
  return c;
}
const totalOf = (c: Record<CatKey, number>) => Object.values(c).reduce((a, b) => a + b, 0);

function CompBar({ c, h = 10 }: { c: Record<CatKey, number>; h?: number }) {
  const total = totalOf(c);
  if (!total) return <div className="text-muted-foreground text-[12.5px]">(暂无快照)</div>;
  return (
    <div className="flex w-full overflow-hidden rounded-full bg-muted" style={{ height: h }}>
      {CAT_KEYS.map((k) =>
        c[k] > 0 ? (
          <div
            key={k}
            title={`${CATS[k].label} ≈${c[k]}`}
            style={{ width: `${(c[k] / total) * 100}%`, background: CATS[k].color }}
          />
        ) : null,
      )}
    </div>
  );
}

const STATUS_STYLE: Record<CtxStep["status"], string> = {
  ok: "bg-emerald-50 text-emerald-700 border-emerald-200",
  error: "bg-red-50 text-red-700 border-red-200",
  cancelled: "bg-amber-50 text-amber-700 border-amber-200",
};
const STATUS_LABEL: Record<CtxStep["status"], string> = {
  ok: "成功",
  error: `失败`,
  cancelled: "已取消",
};

export function ContextView() {
  const [steps, setSteps] = useState<CtxStep[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [auto, setAuto] = useState(true);
  const [onlyCurrent, setOnlyCurrent] = useState(true);
  const [openSeq, setOpenSeq] = useState<number | null>(null);

  const refresh = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.context();
      setSteps(r.steps);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);
  // 自动刷新:8s 一轮(长对话挂着观察上下文增长)
  useEffect(() => {
    if (!auto) return;
    const t = setInterval(() => void refresh(), 8_000);
    return () => clearInterval(t);
  }, [auto, refresh]);

  const sid = localStorage.getItem("bm_session");
  // 时序新→旧;「仅当前会话」默认开(看别的会话可切)
  const visible = useMemo(() => {
    const list = onlyCurrent && sid ? steps.filter((s) => s.session_id === sid) : steps;
    return [...list].reverse();
  }, [steps, onlyCurrent, sid]);

  const latest = visible[0];
  const latestCats = latest ? catSizes(latest) : null;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 px-4 pb-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
        <span className="text-[12.5px] text-muted-foreground">
          每次调用的请求快照与用量;组成占比为估算,真实值以 provider 回报为准
        </span>
        <span className="flex-1" />
        <span className="flex items-center gap-1.5">
          <Switch
            id="ctx-only"
            checked={onlyCurrent && !!sid}
            onCheckedChange={setOnlyCurrent}
            disabled={!sid}
          />
          <Label htmlFor="ctx-only" className="text-[12.5px]">仅当前会话</Label>
        </span>
        <span className="flex items-center gap-1.5">
          <Switch id="ctx-auto" checked={auto} onCheckedChange={setAuto} />
          <Label htmlFor="ctx-auto" className="text-[12.5px]">8s 自动刷新</Label>
        </span>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => void refresh()} data-slot="ctx-refresh">
          {busy ? <Loader2Icon className="animate-spin" /> : <RefreshCwIcon />}
          刷新
        </Button>
      </div>

      {error ? (
        <div className="rounded-lg border border-red-300 bg-red-50 px-3 py-2 text-[12.5px] text-red-700">
          {error}
        </div>
      ) : null}

      {/* 当前上下文(最近一次请求) */}
      <div className="bg-card rounded-xl border p-3" data-slot="ctx-current">
        <div className="mb-2 flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <span className="text-[13.5px] font-semibold">当前上下文(下一次请求同款)</span>
          {latest ? (
            <>
              <span className="text-[12.5px] text-muted-foreground">
                估算合计 ≈{totalOf(latestCats!)} · 实际输入 {latest.tokens_in ?? "—"} / 输出{" "}
                {latest.tokens_out ?? "—"} · 耗时 {fmtDur(latest.latency_ms)}
              </span>
              <span className="flex-1" />
              <span className="text-[12.5px] text-muted-foreground">
                {latest.model_id} · 第 {latest.turn_index} 轮 · 第 {latest.step} 步
              </span>
            </>
          ) : null}
        </div>
        {latest && latestCats ? (
          <>
            <CompBar c={latestCats} h={12} />
            <div className="mt-2 grid grid-cols-2 gap-x-6 gap-y-1 sm:grid-cols-3">
              {CAT_KEYS.map((k) => (
                <div key={k} className="flex items-center gap-1.5 text-[12px]">
                  <span className="inline-block h-2.5 w-2.5 rounded-sm" style={{ background: CATS[k].color }} />
                  <span>{CATS[k].label}</span>
                  <span className="text-muted-foreground">
                    ≈{latestCats[k]} · {totalOf(latestCats) ? Math.round((latestCats[k] / totalOf(latestCats)) * 100) : 0}%
                  </span>
                </div>
              ))}
            </div>
          </>
        ) : (
          <div className="text-muted-foreground py-6 text-center text-[12.5px]" data-slot="ctx-empty">
            {steps.length > 0
              ? `当前会话还没有快照${onlyCurrent && sid ? `(库里有 ${steps.length} 条其他会话记录,可关闭「仅当前会话」查看)` : ""}——发一条消息后这里就能看到发给了模型什么`
              : "还没有快照——发一条消息后这里就能看到发给了模型什么"}
          </div>
        )}
      </div>

      {/* 趋势:每步一根堆叠迷你柱(左→右 = 时间旧→新) */}
      {visible.length > 1 ? (
        <div className="bg-card rounded-xl border p-3">
          <div className="mb-2 text-[13.5px] font-semibold">上下文趋势(每步一柱,左旧右新)</div>
          <div className="flex h-16 items-end gap-1 overflow-x-auto">
            {[...visible].reverse().map((s) => {
              const c = catSizes(s);
              const total = totalOf(c) || 1;
              return (
                <div
                  key={s.seq}
                  className="flex h-full w-4 shrink-0 cursor-pointer flex-col justify-end overflow-hidden rounded-sm"
                  title={`第${s.turn_index}轮·第${s.step}步 · ${s.model_id} · 估算≈${total} · in ${s.tokens_in ?? "—"}/out ${s.tokens_out ?? "—"}${s.status !== "ok" ? ` · ${STATUS_LABEL[s.status]}` : ""}`}
                  onClick={() => setOpenSeq(s.seq)}
                  data-slot="ctx-trend-bar"
                >
                  {CAT_KEYS.map((k) =>
                    c[k] > 0 ? (
                      <div key={k} style={{ height: `${(c[k] / total) * 100}%`, background: CATS[k].color, opacity: s.status === "ok" ? 1 : 0.45 }} />
                    ) : null,
                  )}
                </div>
              );
            })}
          </div>
        </div>
      ) : null}

      {/* 步骤明细(新→旧;点击展开逐项浏览器) */}
      <div className="min-h-0 flex-1 overflow-auto rounded-xl border" data-slot="ctx-steps">
        {visible.length === 0 ? (
          <div className="text-muted-foreground p-6 text-center text-[12.5px]">
            {steps.length > 0
              ? `当前会话暂无快照(另有 ${steps.length} 条其他会话记录;可关闭「仅当前会话」查看)`
              : "(无快照)"}
          </div>
        ) : (
          visible.map((s) => {
            const c = catSizes(s);
            const open = openSeq === s.seq;
            const time = (() => {
              const d = new Date(s.ts);
              return isNaN(d.getTime()) ? s.ts : d.toLocaleTimeString();
            })();
            return (
              <div key={s.seq} className="border-b last:border-b-0">
                <button
                  className="flex w-full flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-left hover:bg-muted/50"
                  onClick={() => setOpenSeq(open ? null : s.seq)}
                  data-slot="ctx-step-head"
                  data-seq={s.seq}
                >
                  <span className="font-mono text-[12px]">#{s.seq}</span>
                  <span className="text-[12.5px]">
                    第 {s.turn_index} 轮 · 第 {s.step} 步
                    {s.attempt && s.attempt > 1 ? ` · 尝试 ${s.attempt}` : ""}
                  </span>
                  <span className="text-[12.5px] text-muted-foreground">{time}</span>
                  <span className={`rounded border px-1.5 py-0.5 text-[11px] ${STATUS_STYLE[s.status]}`}>
                    {s.status === "error" && s.error_code ? `${STATUS_LABEL[s.status]} ${s.error_code}` : STATUS_LABEL[s.status]}
                  </span>
                  <span className="flex-1" />
                  <span className="text-[12px] text-muted-foreground">
                    实际 in {s.tokens_in ?? "—"} / out {s.tokens_out ?? "—"} · 耗时 {fmtDur(s.latency_ms)} · 估算 ≈
                    {totalOf(c)} · {s.model_id}
                  </span>
                </button>
                {open ? (
                  <div className="bg-muted/30 px-3 pb-3 pt-1">
                    <div className="mb-1.5 text-[12.5px] font-semibold">请求组成(即模型实际收到的内容)</div>
                    <div className="flex flex-col gap-1.5">
                      {(s.messages ?? []).map((m, i) => (
                        <details key={i} className="bg-card rounded-lg border">
                          <summary className="flex cursor-pointer flex-wrap items-center gap-2 px-2.5 py-1.5 text-[12.5px]">
                            <span className="inline-block h-2.5 w-2.5 rounded-sm" style={{ background: CATS[m.role === "system" ? "system" : m.role === "user" ? "user" : m.role === "assistant" ? "assistant" : "toolres"].color }} />
                            <span className="font-medium">{ROLE_LABEL[m.role] ?? m.role}</span>
                            <span className="text-muted-foreground">≈{est(m.content)} · {m.content?.length ?? 0} 字符</span>
                            {m.content_truncated ? <span className="text-amber-600">(快照截断)</span> : null}
                          </summary>
                          <pre className="max-h-64 overflow-auto border-t px-2.5 py-2 font-mono text-[11.5px] leading-relaxed break-all whitespace-pre-wrap">
                            {m.content}
                          </pre>
                        </details>
                      ))}
                      {(s.tools ?? []).length > 0 ? (
                        <details className="bg-card rounded-lg border">
                          <summary className="cursor-pointer px-2.5 py-1.5 text-[12.5px]">
                            <span className="font-medium">工具定义 × {s.tools.length}</span>
                            <span className="text-muted-foreground"> ≈{c.tools}</span>
                          </summary>
                          <div className="border-t px-2.5 py-2">
                            {(s.tools ?? []).map((t, i) => (
                              <details key={i} className="mb-1">
                                <summary className="cursor-pointer text-[12px]">
                                  <span className="font-mono">{t.function?.name ?? `#${i + 1}`}</span>
                                  <span className="text-muted-foreground">{t.function?.description ? ` — ${t.function.description}` : ""}</span>
                                </summary>
                                <pre className="bg-muted/40 mt-1 max-h-48 overflow-auto rounded p-2 font-mono text-[11px]">
                                  {JSON.stringify(t, null, 2)}
                                </pre>
                              </details>
                            ))}
                          </div>
                        </details>
                      ) : null}
                    </div>
                  </div>
                ) : null}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
