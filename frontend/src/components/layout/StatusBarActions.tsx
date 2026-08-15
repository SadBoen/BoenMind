/**
 * 面板标题栏右侧统一状态区（用户拍板"把 X 关闭那一行做成状态栏"，2026-08-15）：
 * dockview 8.1 header action 槽位（rightHeaderActionsComponent）——每个面板组
 * 标题栏右侧挂本组件，按组内活跃面板渲染状态。
 *
 * 当前状态项：对话面板的会话 token 用量（数据源 /api/sessions/{id}/usage——
 * 事件日志 assistant/message 事件 usage 聚合）。宽度不足时图标 + 总量即可，
 * 明细（输入/输出/消息数）点开悬浮窗（用户拍板"右状态悬浮"）。
 * 后续状态项（缓存命中率等）在此追加。
 */
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { Coins, Loader2, Menu } from "lucide-react";
import type { IDockviewHeaderActionsProps } from "dockview-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { api } from "@/api/client";
import { useAppStore } from "@/stores/app-store";

/** token 数缩写：<1K 原样；<1M → x.xK；否则 x.xM */
function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}K`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

interface SessionUsage {
  input_tokens: number;
  output_tokens: number;
  messages: number;
}

/** 对话面板 token 用量：图标 + 总量；点击悬浮窗看明细 */
function TokenUsageButton() {
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

  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        className="h-6 gap-1 px-1.5 text-[10px] font-normal text-muted-foreground"
        title={t("statusbar.tokenUsage")}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setAnchor({ x: r.right, y: r.bottom });
          setOpen((o) => !o);
        }}
      >
        <Coins size={12} />
        {usage ? formatTokens(total) : <Loader2 size={10} className="animate-spin" />}
      </Button>
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
            </div>
          </>,
          document.body,
        )
      )}
    </>
  );
}

/**
 * 统一状态区宿主：按组内活跃面板的视图类型渲染状态项。
 * 对话面板（chat-pane 视图）→ token 用量；其他面板暂无状态项（留扩展位）。
 */
export function StatusBarActions({ activePanel }: IDockviewHeaderActionsProps) {
  if (activePanel?.view.contentComponent !== "chat-pane") return null;
  return <TokenUsageButton />;
}

/**
 * 状态栏最左槽位（prefix）：聊天单元内嵌会话列表的显隐开关。
 * 用户拍板"列表在聊天单元内部，由顶部状态栏控制"（2026-08-15）——
 * 三横按钮与 token 状态、关闭按钮同一行（状态栏统一承载）。
 * 仅对话面板组显示；按场景（chat/coding）记忆显隐，聊天应用默认展开。
 */
export function SessionsToggle({ activePanel }: IDockviewHeaderActionsProps) {
  const { t } = useTranslation();
  const params = (activePanel?.params ?? {}) as { app?: string };
  const scene = params.app ?? "chat";
  const open = useAppStore((s) => s.chatSessionsOpen[scene] ?? scene === "chat");
  const toggle = useAppStore((s) => s.toggleChatSessions);
  if (activePanel?.view.contentComponent !== "chat-pane") return null;
  return (
    <Button
      variant="ghost"
      size="sm"
      className={cn(
        "h-6 w-6 px-0 text-muted-foreground",
        open && "bg-accent text-foreground",
      )}
      title={t("chat.sessions")}
      onClick={() => toggle(scene)}
    >
      <Menu size={14} />
    </Button>
  );
}
