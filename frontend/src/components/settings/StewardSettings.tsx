/**
 * 管家（Steward）设置：状态展示 + 手动汇报注入（架构 §14.5 续接）。
 *
 * 管家 = 无人值守下自主运行的代理会话（BM_STEWARD_SESSION env 指定）：
 * - 状态卡片：启用会话 / 下次唤醒 / 上次回合 / 登记原因 / 治理夹区间；
 * - 汇报入口：OS 层事件 → 立即投喂一个 Inject 回合（可带下次唤醒秒数）。
 * 状态每 5s 轮询（回合进行中 inFlight 徽标实时可见）。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Activity, RefreshCw, Send } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { toast } from "sonner";
import { api, type StewardStatus } from "@/api/client";

export function StewardSettings() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<StewardStatus | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);

  const load = async () => {
    try {
      setStatus(await api.stewardStatus());
    } catch (err) {
      toast.error(t("settings.steward.loadFailed", { error: String(err) }));
    }
  };

  // 5s 轮询：管家回合进行中 inFlight 徽标实时可见（卸载即停）
  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(), 5000);
    return () => clearInterval(timer);
  }, []);

  const inject = async () => {
    const text = message.trim();
    if (!text) return;
    setBusy(true);
    try {
      await api.stewardInject(text);
      toast.success(t("settings.steward.injected"));
      setMessage("");
      await load();
    } catch (err) {
      toast.error(t("settings.steward.injectFailed", { error: String(err) }));
    } finally {
      setBusy(false);
    }
  };

  const fmt = (ms?: number) =>
    ms ? new Date(ms).toLocaleString() : t("settings.steward.silent");

  const countdown = () => {
    if (!status?.nextWakeAtMs) return null;
    const secs = Math.max(0, Math.round((status.nextWakeAtMs - Date.now()) / 1000));
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return m > 0 ? `${m}m${s}s` : `${s}s`;
  };

  return (
    <div className="space-y-5">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Activity size={16} />
        <p>{t("settings.steward.hint")}</p>
      </div>

      <div className="rounded-lg border bg-card p-4">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-sm font-semibold">{t("settings.steward.title")}</h3>
          <div className="flex items-center gap-2">
            {status?.inFlight && (
              <Badge variant="secondary">{t("settings.steward.inFlight")}</Badge>
            )}
            <Button size="sm" variant="ghost" onClick={() => void load()}>
              <RefreshCw size={14} /> {t("settings.steward.refresh")}
            </Button>
          </div>
        </div>

        {!status?.enabled ? (
          <p className="mt-3 text-sm text-muted-foreground">
            {t("settings.steward.disabled")}
          </p>
        ) : (
          <dl className="mt-3 grid grid-cols-1 gap-x-6 gap-y-2 text-sm sm:grid-cols-2">
            <div className="flex items-center justify-between gap-2">
              <dt className="text-muted-foreground">{t("settings.steward.session")}</dt>
              <dd className="font-mono text-xs">{status.sessionId}</dd>
            </div>
            <div className="flex items-center justify-between gap-2">
              <dt className="text-muted-foreground">{t("settings.steward.nextWake")}</dt>
              <dd>
                {fmt(status.nextWakeAtMs)}
                {countdown() != null && (
                  <span className="ml-1 text-xs text-muted-foreground">
                    ({countdown()})
                  </span>
                )}
              </dd>
            </div>
            <div className="flex items-center justify-between gap-2">
              <dt className="text-muted-foreground">{t("settings.steward.lastWake")}</dt>
              <dd>{fmt(status.lastWakeAtMs)}</dd>
            </div>
            <div className="flex items-center justify-between gap-2">
              <dt className="text-muted-foreground">{t("settings.steward.reason")}</dt>
              <dd className="max-w-[55%] truncate" title={status.lastReason ?? ""}>
                {status.lastReason ?? "—"}
              </dd>
            </div>
            <div className="flex items-center justify-between gap-2">
              <dt className="text-muted-foreground">{t("settings.steward.pacing")}</dt>
              <dd>
                {status.pacingMinS}s ~ {status.pacingMaxS}s
              </dd>
            </div>
          </dl>
        )}
      </div>

      {/* 手动汇报注入（OS 层事件通道：POST /api/steward/inject） */}
      <div className="rounded-lg border bg-card p-4">
        <h3 className="text-sm font-semibold">{t("settings.steward.injectTitle")}</h3>
        <textarea
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          placeholder={t("settings.steward.injectPlaceholder")}
          rows={3}
          className="mt-3 w-full rounded-md border bg-background px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary/50"
        />
        <div className="mt-2 flex justify-end">
          <Button
            size="sm"
            disabled={busy || !message.trim()}
            onClick={() => void inject()}
          >
            <Send size={14} /> {t("settings.steward.inject")}
          </Button>
        </div>
      </div>
    </div>
  );
}
