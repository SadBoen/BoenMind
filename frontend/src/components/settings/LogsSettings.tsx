/**
 * 设置中心「日志」页：查看 bm-server 内存环形日志（最近 5000 条），
 * 支持最低级别 + 关键字筛选，自动刷新（默认开，5s）。
 * 日志由后端 tracing BufferLayer 收集（log_buffer.rs），进程内不落盘。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { RefreshCw, Search, X } from "lucide-react";
import { api, type LogEntry } from "@/api/client";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";

const LEVELS = ["", "trace", "debug", "info", "warn", "error"] as const;
const LEVEL_STYLE: Record<string, string> = {
  TRACE: "bg-zinc-500/15 text-zinc-400",
  DEBUG: "bg-sky-500/15 text-sky-600 dark:text-sky-400",
  INFO: "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400",
  WARN: "bg-amber-500/15 text-amber-600 dark:text-amber-400",
  ERROR: "bg-red-500/15 text-red-600 dark:text-red-400",
};

const REFRESH_MS = 5000;
const PAGE_SIZE = 200;

function formatTime(tsMs: number): string {
  const d = new Date(tsMs);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export function LogsSettings() {
  const { t } = useTranslation();
  const [level, setLevel] = useState<string>("");
  const [keyword, setKeyword] = useState("");
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);

  const load = useCallback(
    async (scrollTop: boolean) => {
      setLoading(true);
      try {
        const data = await api.logs({
          level: level || undefined,
          q: keyword.trim() || undefined,
          limit: PAGE_SIZE,
        });
        setEntries(data.entries);
        setError(null);
        // 列表顶部总是"最新"（倒序返回）；用户向上滚到顶时才跟随新日志
        if (scrollTop && listRef.current && listRef.current.scrollTop <= 8) {
          listRef.current.scrollTop = 0;
        }
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [level, keyword],
  );

  // 手动刷新（筛选项变化时重置到顶部）
  const refresh = () => void load(true);

  // 自动刷新：仅当用户停留在列表顶部（未向下翻看）时滚动跟随
  useEffect(() => {
    if (!autoRefresh) return;
    const timer = setInterval(() => void load(false), REFRESH_MS);
    return () => clearInterval(timer);
  }, [autoRefresh, load]);

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [level, keyword]);

  const onScroll = () => {
    const el = listRef.current;
    if (!el) return;
    setPaused(el.scrollTop > 8);
  };

  return (
    <section className="flex h-full min-h-0 flex-col gap-4">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold">{t("settings.logs.title")}</h2>
          <p className="text-sm text-muted-foreground">{t("settings.logs.desc")}</p>
        </div>
        <div className="flex items-center gap-3">
          <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
            {t("settings.logs.autoRefresh")}
            <Switch checked={autoRefresh} onCheckedChange={setAutoRefresh} size="sm" />
          </label>
          <Button variant="outline" size="sm" onClick={refresh} disabled={loading}>
            <RefreshCw size={14} className={`mr-1 ${loading ? "animate-spin" : ""}`} />
            {t("settings.logs.refresh")}
          </Button>
        </div>
      </div>

      {/* 筛选栏：级别 + 关键字 */}
      <div className="flex flex-wrap items-center gap-2">
        <select
          value={level}
          onChange={(e) => setLevel(e.target.value)}
          className="h-8 rounded-md border bg-background px-2 text-xs"
          aria-label={t("settings.logs.levelLabel")}
        >
          <option value="">{t("settings.logs.levelAll")}</option>
          {LEVELS.slice(1).map((l) => (
            <option key={l} value={l}>
              {l.toUpperCase()}
            </option>
          ))}
        </select>
        <div className="relative min-w-[12rem] flex-1">
          <Search size={13} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
            placeholder={t("settings.logs.keywordPlaceholder")}
            className="h-8 pl-7 text-xs"
          />
          {keyword && (
            <button
              type="button"
              onClick={() => setKeyword("")}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
              aria-label={t("common.clear")}
            >
              <X size={13} />
            </button>
          )}
        </div>
        <span className="text-xs text-muted-foreground">
          {t("settings.logs.count", { count: entries.length })}
        </span>
      </div>

      {error && <p className="text-xs text-red-500">{t("settings.logs.loadFailed", { error })}</p>}

      {/* 日志列表（独立滚动区，最新在上） */}
      <div
        ref={listRef}
        onScroll={onScroll}
        className="min-h-0 flex-1 overflow-y-auto rounded-md border bg-muted/40 font-mono text-[11px] leading-relaxed"
      >
        {entries.length === 0 ? (
          <p className="p-4 text-muted-foreground">{t("settings.logs.empty")}</p>
        ) : (
          entries.map((e, i) => (
            <div
              key={`${e.ts_ms}-${i}`}
              className="flex gap-2 border-b border-border/50 px-3 py-1"
            >
              <span className="shrink-0 tabular-nums text-muted-foreground">
                {formatTime(e.ts_ms)}
              </span>
              <span
                className={`w-14 shrink-0 rounded px-1 text-center font-semibold ${LEVEL_STYLE[e.level] ?? "bg-muted text-muted-foreground"}`}
              >
                {e.level}
              </span>
              <span className="shrink-0 max-w-[16rem] truncate text-muted-foreground/70" title={e.target}>
                {e.target}
              </span>
              <span className="min-w-0 break-all whitespace-pre-wrap">{e.message}</span>
            </div>
          ))
        )}
      </div>
      <p className="text-[11px] text-muted-foreground">
        {paused ? t("settings.logs.scrolledHint") : t("settings.logs.following")}
      </p>
    </section>
  );
}
