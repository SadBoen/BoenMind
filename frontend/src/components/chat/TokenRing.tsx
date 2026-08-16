/**
 * 输入框旁的会话 token 用量圆环（2026-08-17 用户拍板：替代右上角 682.6K
 * 按钮，参照 hermes-webui 的 ctx 环）。数据源 /api/sessions/{id}/usage
 * （事件日志 assistant/message 事件 usage 聚合，与旧 TokenUsageButton 同源）。
 *
 * 百分比 = 会话累计 token / 参考窗口 128K（与后端压缩默认窗口一致——
 * 压缩配置 overrides 可按 provider/model 覆盖，前端暂无逐模型窗口，
 * 以 128K 作统一参考）；>100% 时环满。点击弹出明细（输入/输出/总量/消息数）。
 */
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/app-store";
import { api } from "@/api/client";

/** 参考上下文窗口（与后端 compaction 默认窗口一致） */
const REF_WINDOW = 128_000;

interface SessionUsage {
  input_tokens: number;
  output_tokens: number;
  messages: number;
}

/** token 数缩写：<1K 原样；<1M → x.xK；否则 x.xM */
function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}K`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

export function TokenRing() {
  const { t } = useTranslation();
  const activeSessionId = useAppStore((s) => s.activeSessionId);
  // 流结束时 +1（store 在 finalizeStream 递增）→ 重新拉取
  const usageVersion = useAppStore((s) => s.usageVersion);
  const [usage, setUsage] = useState<SessionUsage | null>(null);
  const [open, setOpen] = useState(false);
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    if (!activeSessionId) {
      setUsage(null);
      return;
    }
    let cancelled = false;
    api
      .getSessionUsage(activeSessionId)
      .then((u) => {
        if (!cancelled) setUsage(u);
      })
      .catch(() => {
        if (!cancelled) setUsage(null);
      });
    return () => {
      cancelled = true;
    };
  }, [activeSessionId, usageVersion]);

  if (!activeSessionId) return null;
  const total = usage ? usage.input_tokens + usage.output_tokens : 0;
  const pct = Math.min(100, Math.round((total / REF_WINDOW) * 100));
  // SVG 圆环：r=7 周长 ≈ 43.98
  const C = 2 * Math.PI * 7;
  const dash = (pct / 100) * C;

  return (
    <>
      <button
        type="button"
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setAnchor({ x: r.right, y: r.bottom });
          setOpen((o) => !o);
        }}
        title={t("statusbar.tokenUsage")}
        className="flex h-7 w-7 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-accent"
      >
        <svg width="17" height="17" viewBox="0 0 18 18" aria-hidden="true">
          <circle cx="9" cy="9" r="7" fill="none" stroke="currentColor" strokeOpacity="0.15" strokeWidth="2.5" />
          <circle
            cx="9"
            cy="9"
            r="7"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeDasharray={`${dash} ${C - dash}`}
            transform="rotate(-90 9 9)"
            className={usage ? "text-primary" : "text-muted-foreground"}
          />
        </svg>
      </button>
      {open && anchor && (
        createPortal(
          <>
            <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
            <div
              className="fixed z-50 w-64 rounded-lg border bg-popover p-3 text-xs shadow-md"
              style={{ right: Math.min(window.innerWidth - anchor.x + 8, 16), top: anchor.y + 6 }}
            >
              <p className="mb-2 font-medium">{t("statusbar.tokenUsage")}</p>
              <dl className="grid grid-cols-2 gap-y-1.5 text-muted-foreground">
                <dt>{t("statusbar.inputTokens")}</dt>
                <dd className="text-right font-mono">{usage ? usage.input_tokens.toLocaleString() : "—"}</dd>
                <dt>{t("statusbar.outputTokens")}</dt>
                <dd className="text-right font-mono">{usage ? usage.output_tokens.toLocaleString() : "—"}</dd>
                <dt>{t("statusbar.totalTokens")}</dt>
                <dd className="text-right font-mono">{usage ? total.toLocaleString() : "—"}</dd>
                <dt>{t("statusbar.messages")}</dt>
                <dd className="text-right font-mono">{usage ? usage.messages.toLocaleString() : "—"}</dd>
              </dl>
              <p className="mt-2 text-[10px] text-muted-foreground/70">
                {formatTokens(total)} / {formatTokens(REF_WINDOW)}
              </p>
            </div>
          </>,
          document.body,
        )
      )}
    </>
  );
}
