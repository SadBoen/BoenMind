/**
 * 作用域选择器（设置架构 §八）：扩展（插件/SKILL）生效的 APP 范围。
 * 公共（默认）→ 所有 APP；仅聊天 / 仅编程 → 只进该 APP 的会话工具面/注入面。
 *
 * 语义：空/["*"] = 公共；["chat"] / ["coding"] = 仅绑定该 APP（单选——
 * 当前一个扩展绑定一个 APP 或公共）。MCP server 的作用域在配置表单里，
 * 与此处同一套取值。
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Globe, MessageSquare, Code2 } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";

/** 归一化：空/含 "*" → []（公共） */
export function normalizeScopes(scopes?: string[]): string[] {
  if (!scopes || scopes.length === 0) return [];
  const set = new Set(scopes.filter((s) => s && s !== "*"));
  return [...set];
}

/** 当前生效 APP（公共 = null） */
export function effectiveApp(scopes?: string[]): string | null {
  const norm = normalizeScopes(scopes);
  return norm.length === 0 ? null : norm[0];
}

const SCOPE_OPTIONS = [
  { value: null, icon: <Globe size={15} />, labelKey: "settings.scope.public" },
  { value: "chat", icon: <MessageSquare size={15} />, labelKey: "settings.scope.chat" },
  { value: "coding", icon: <Code2 size={15} />, labelKey: "settings.scope.coding" },
] as const;

/** 列表行徽标：作用域 → 公共/聊天/编程 */
export function ScopeBadge({ scopes }: { scopes?: string[] }) {
  const { t } = useTranslation();
  const app = effectiveApp(scopes);
  const labelKey = app
    ? `settings.scope.badge.${app}`
    : "settings.scope.badge.public";
  return (
    <Badge variant="outline" className="text-[10px] font-normal">
      {t(labelKey)}
    </Badge>
  );
}

/**
 * 作用域编辑对话框 + 触发按钮（列表行的编辑入口）。
 * `current` 当前作用域；`onSave` 保存后刷新列表（后端已持久化）。
 */
export function ScopePicker({
  name,
  current,
  onSave,
}: {
  name: string;
  current: string[] | undefined;
  onSave: (scopes: string[]) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);

  const openPicker = () => {
    setSelected(effectiveApp(current));
    setOpen(true);
  };

  const save = async () => {
    setSaving(true);
    try {
      await onSave(selected ? [selected] : []);
      toast.success(t("settings.scope.saved", { name }));
      setOpen(false);
    } catch (err) {
      toast.error(t("settings.scope.saveFailed", { error: String(err) }));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="shrink-0"
        title={t("settings.scope.title")}
        onClick={openPicker}
      >
        <Globe size={14} />
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("settings.scope.title", { name })}</DialogTitle>
            <DialogDescription>{t("settings.scope.desc")}</DialogDescription>
          </DialogHeader>
          <div className="space-y-2">
            {SCOPE_OPTIONS.map((opt) => {
              const active = selected === opt.value;
              return (
                <button
                  key={opt.labelKey}
                  type="button"
                  onClick={() => setSelected(opt.value)}
                  className={cn(
                    "flex w-full items-center gap-2.5 rounded-lg border-2 p-3 text-left text-sm transition-colors",
                    active ? "border-primary bg-primary/5" : "border-border hover:border-muted-foreground/40",
                  )}
                >
                  {opt.icon}
                  <span className="font-medium">{t(opt.labelKey)}</span>
                </button>
              );
            })}
          </div>
          <DialogFooter>
            <Button onClick={() => void save()} disabled={saving}>
              {t("common.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
