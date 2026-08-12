/**
 * 插件设置表单控件：CollapsibleCard（折叠卡片）+ SettingFieldInput（按类型渲染）。
 * 类型：string / secret（掩码回显 + 清除标记）/ boolean / number / select。
 */
import { useTranslation } from "react-i18next";
import { ChevronDown, KeyRound, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { SettingField } from "@/api/client";

export type SettingValue = string | number | boolean;

export function CollapsibleCard({
  title,
  isOpen,
  onToggle,
  actions,
  children,
}: {
  title: string;
  isOpen: boolean;
  onToggle: () => void;
  actions?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="rounded-xl border">
      <div className="flex items-center gap-2 px-3 py-2.5">
        <button
          type="button"
          onClick={onToggle}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          <ChevronDown
            size={14}
            className={`shrink-0 text-muted-foreground transition-transform ${isOpen ? "" : "-rotate-90"}`}
          />
          <span className="truncate text-sm font-semibold">{title}</span>
        </button>
        {actions && <div className="flex shrink-0 items-center gap-1.5">{actions}</div>}
      </div>
      {isOpen && <div className="border-t px-4 py-3">{children}</div>}
    </div>
  );
}

export function SettingFieldInput({
  field,
  value,
  onChange,
  disabled,
  cleared,
  onToggleClear,
}: {
  field: SettingField;
  value: SettingValue | undefined;
  onChange: (v: SettingValue) => void;
  disabled?: boolean;
  /** secret 字段：已标记待清除（保存时删除密钥） */
  cleared?: boolean;
  onToggleClear?: () => void;
}) {
  const { t } = useTranslation();

  switch (field.type) {
    case "boolean":
      return (
        <div className="flex items-center justify-between gap-3 rounded-lg border p-3">
          <div className="min-w-0">
            <p className="text-sm font-medium">{field.label}</p>
            {field.description && (
              <p className="text-xs text-muted-foreground">{field.description}</p>
            )}
          </div>
          <Switch checked={Boolean(value)} onCheckedChange={onChange} disabled={disabled} />
        </div>
      );

    case "number":
      return (
        <div className="space-y-1.5">
          <Label>{field.label}</Label>
          <Input
            type="number"
            min={field.min}
            max={field.max}
            value={String(value ?? field.default ?? "")}
            onChange={(e) => onChange(e.target.value === "" ? "" : Number(e.target.value))}
            disabled={disabled}
          />
          {field.description && (
            <p className="text-xs text-muted-foreground">{field.description}</p>
          )}
        </div>
      );

    case "select":
      return (
        <div className="space-y-1.5">
          <Label>{field.label}</Label>
          <Select
            value={String(value ?? field.default ?? "")}
            onValueChange={(v) => {
              if (v !== null) onChange(v);
            }}
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(field.options ?? []).map((opt) => (
                <SelectItem key={opt} value={opt}>
                  {opt}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {field.description && (
            <p className="text-xs text-muted-foreground">{field.description}</p>
          )}
        </div>
      );

    case "secret": {
      const raw = String(value ?? "");
      // 掩码回显（如 jina****）= 已配置：输入框留空，掩码显示在 placeholder
      const configured = raw.length > 0 && raw.includes("****");
      const isCleared = Boolean(cleared);
      return (
        <div className="space-y-1.5">
          <div className="flex items-center justify-between gap-2">
            <Label>
              {field.label}
              <KeyRound size={11} className="ml-1 inline text-muted-foreground" />
            </Label>
            {configured && !isCleared && (
              <Button
                variant="ghost"
                size="sm"
                className="h-6 gap-1 px-2 text-xs text-muted-foreground hover:text-destructive"
                onClick={onToggleClear}
                disabled={disabled}
              >
                <X size={11} />
                {t("settings.plugins.secretClear")}
              </Button>
            )}
            {isCleared && (
              <span className="text-xs text-destructive">{t("settings.plugins.secretClearing")}</span>
            )}
          </div>
          <Input
            type="password"
            value={configured || isCleared ? "" : raw}
            onChange={(e) => onChange(e.target.value)}
            placeholder={
              configured && !isCleared
                ? t("settings.plugins.secretConfigured", { mask: raw })
                : t("settings.plugins.secretPlaceholder")
            }
            disabled={disabled}
            autoComplete="off"
          />
          {field.description && (
            <p className="text-xs text-muted-foreground">{field.description}</p>
          )}
        </div>
      );
    }

    // string
    default:
      return (
        <div className="space-y-1.5">
          <Label>{field.label}</Label>
          <Input
            type="text"
            value={String(value ?? field.default ?? "")}
            onChange={(e) => onChange(e.target.value)}
            disabled={disabled}
          />
          {field.description && (
            <p className="text-xs text-muted-foreground">{field.description}</p>
          )}
        </div>
      );
  }
}
