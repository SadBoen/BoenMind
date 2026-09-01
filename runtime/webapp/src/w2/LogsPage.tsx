// W2 设置中心 · 日志:数据目录两份 jsonl(execution-log / events)尾部直读,
// 供诊断「工具调用卡死」一类运行期问题(2026-09-02 用户要求接入日志)。
// 默认 5s 自动刷新,可在长回合中挂着观察;行原始 JSON,换行按条分隔。
import { useCallback, useEffect, useRef, useState } from "react";
import { RefreshCwIcon, Loader2Icon } from "lucide-react";
import { api } from "./api";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";

type Which = "exec" | "events";

const TABS: { key: Which; label: string; hint: string }[] = [
  { key: "exec", label: "执行日志", hint: "回合/工具调用明细" },
  { key: "events", label: "事件流", hint: "含 capability.invoked 状态" },
];

export function LogsPage() {
  const [which, setWhich] = useState<Which>("exec");
  const [lines, setLines] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [auto, setAuto] = useState(true);
  const boxRef = useRef<HTMLPreElement>(null);

  const refresh = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.logs();
      setLines(which === "exec" ? r.exec : r.events);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  }, [which]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // 自动刷新:5s 一轮;刷新后贴底(看日志习惯看最新)
  useEffect(() => {
    if (!auto) return;
    const t = setInterval(() => void refresh(), 5_000);
    return () => clearInterval(t);
  }, [auto, refresh]);

  useEffect(() => {
    const box = boxRef.current;
    if (box) box.scrollTop = box.scrollHeight;
  }, [lines]);

  return (
    <div className="flex min-h-0 flex-col gap-4">
      <div>
        <h2 className="text-[15px] font-semibold">日志</h2>
        <p className="text-muted-foreground text-[12.5px]">
          直读数据目录的 execution-log.jsonl 与 events.jsonl 尾部(各 200 行);
          工具调用卡住时,看「事件流」里最后一条 capability.invoked 停在
          intent 还是 error。
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        {TABS.map((t) => (
          <Button
            key={t.key}
            size="sm"
            variant={which === t.key ? "default" : "outline"}
            title={t.hint}
            onClick={() => setWhich(t.key)}
            data-slot="logs-tab"
            data-tab={t.key}
          >
            {t.label}
          </Button>
        ))}
        <span className="flex-1" />
        <span className="flex items-center gap-1.5">
          <Switch id="logs-auto" checked={auto} onCheckedChange={setAuto} />
          <Label htmlFor="logs-auto" className="text-[12.5px]">
            5s 自动刷新
          </Label>
        </span>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => void refresh()}>
          {busy ? <Loader2Icon className="animate-spin" /> : <RefreshCwIcon />}
          刷新
        </Button>
      </div>

      {error ? (
        <div className="rounded-lg border border-red-300 bg-red-50 px-3 py-2 text-[12.5px] text-red-700">
          {error}
        </div>
      ) : null}

      <pre
        ref={boxRef}
        data-slot="logs-box"
        className="bg-card max-h-[62vh] min-h-0 flex-1 overflow-auto rounded-xl border p-3 font-mono text-[11.5px] leading-relaxed break-all whitespace-pre-wrap"
      >
        {lines.length ? lines.join("\n") : "(空——尚无日志)"}
      </pre>
    </div>
  );
}
