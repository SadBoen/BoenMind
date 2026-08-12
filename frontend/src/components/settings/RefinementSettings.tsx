/**
 * 改进建议设置：代理完成任务后提交的 skill 描述/系统提示词改进建议。
 *
 * 审批模式（借鉴 Prime Agent /refine 的"宿主审批"变体）：
 * - pending 建议可 批准（生效：改 SKILL.md 描述 / 追加系统提示词，改前备份）或 拒绝；
 * - 已批准且带备份的 skill 建议可 还原（一键回滚到审批前状态）。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, CornerUpLeft, Lightbulb, Loader2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { toast } from "sonner";
import { api, type RefinementSuggestion } from "@/api/client";

type Filter = "all" | "pending" | "approved" | "rejected";

const FILTERS: Filter[] = ["all", "pending", "approved", "rejected"];

export function RefinementSettings() {
  const { t } = useTranslation();
  const [items, setItems] = useState<RefinementSuggestion[]>([]);
  const [filter, setFilter] = useState<Filter>("all");
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);

  const load = async () => {
    setLoading(true);
    try {
      setItems(await api.listRefinementSuggestions());
    } catch (err) {
      toast.error(t("settings.refinement.loadFailed", { error: String(err) }));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const visible = items.filter((it) => filter === "all" || it.status === filter);

  const approve = async (it: RefinementSuggestion) => {
    if (!window.confirm(t("settings.refinement.approveConfirm"))) return;
    setBusyId(it.id);
    try {
      const res = await api.approveRefinementSuggestion(it.id);
      toast.success(
        it.target.startsWith("skill:")
          ? t("settings.refinement.appliedSkill")
          : t("settings.refinement.appliedPrompt"),
        {
          description: res.backup
            ? t("settings.refinement.backupHint")
            : undefined,
        },
      );
      await load();
    } catch (err) {
      toast.error(t("settings.refinement.approveFailed", { error: String(err) }));
    } finally {
      setBusyId(null);
    }
  };

  const reject = async (it: RefinementSuggestion) => {
    if (!window.confirm(t("settings.refinement.rejectConfirm"))) return;
    setBusyId(it.id);
    try {
      await api.rejectRefinementSuggestion(it.id);
      toast.success(t("settings.refinement.rejected"));
      await load();
    } catch (err) {
      toast.error(t("settings.refinement.rejectFailed", { error: String(err) }));
    } finally {
      setBusyId(null);
    }
  };

  const rollback = async (it: RefinementSuggestion) => {
    if (!window.confirm(t("settings.refinement.rollbackConfirm"))) return;
    setBusyId(it.id);
    try {
      await api.rollbackRefinementSuggestion(it.id);
      toast.success(t("settings.refinement.rolledBack"));
      await load();
    } catch (err) {
      toast.error(t("settings.refinement.rollbackFailed", { error: String(err) }));
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Lightbulb size={16} />
        <p>{t("settings.refinement.hint")}</p>
      </div>

      {/* 状态过滤 */}
      <div className="flex gap-1.5">
        {FILTERS.map((f) => (
          <button
            key={f}
            onClick={() => setFilter(f)}
            className={`rounded-md px-3 py-1 text-xs transition-colors ${
              filter === f
                ? "bg-primary text-primary-foreground"
                : "bg-muted text-muted-foreground hover:bg-muted/70"
            }`}
          >
            {t(`settings.refinement.filter.${f}`)}
          </button>
        ))}
      </div>

      {loading ? (
        <div className="flex justify-center py-10 text-muted-foreground">
          <Loader2 className="animate-spin" size={20} />
        </div>
      ) : visible.length === 0 ? (
        <p className="py-10 text-center text-sm text-muted-foreground">
          {t("settings.refinement.empty")}
        </p>
      ) : (
        <ul className="space-y-3">
          {visible.map((it) => (
            <li key={it.id} className="rounded-lg border bg-card p-4">
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                  <span className="font-mono">{it.target}</span>
                  <span>
                    {new Date(it.created_at * 1000).toLocaleString()}
                  </span>
                </div>
                <Badge
                  variant={
                    it.status === "approved"
                      ? "default"
                      : it.status === "rejected"
                        ? "secondary"
                        : "outline"
                  }
                >
                  {t(`settings.refinement.status.${it.status}`)}
                </Badge>
              </div>

              <blockquote className="mt-3 rounded-md border-l-2 border-destructive/40 bg-muted/40 px-3 py-2 text-sm text-muted-foreground">
                {it.quote}
              </blockquote>
              <div className="mt-1.5 rounded-md border-l-2 border-primary/50 bg-primary/5 px-3 py-2 text-sm">
                {it.suggested}
              </div>
              <p className="mt-2 text-xs text-muted-foreground">
                {t("settings.refinement.reason")}：{it.reason}
              </p>

              {it.status === "pending" && (
                <div className="mt-3 flex gap-2">
                  <Button
                    size="sm"
                    disabled={busyId === it.id}
                    onClick={() => void approve(it)}
                  >
                    <Check size={14} /> {t("settings.refinement.approve")}
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busyId === it.id}
                    onClick={() => void reject(it)}
                  >
                    <X size={14} /> {t("settings.refinement.reject")}
                  </Button>
                </div>
              )}
              {it.status === "approved" && it.backup_path && (
                <div className="mt-3">
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busyId === it.id}
                    onClick={() => void rollback(it)}
                  >
                    <CornerUpLeft size={14} /> {t("settings.refinement.rollback")}
                  </Button>
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
