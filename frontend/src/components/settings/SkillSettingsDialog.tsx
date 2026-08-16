/**
 * SKILL 设置对话框：skill 目录 `settings.json` 声明的 schema → 动态表单。
 * 与插件设置同构（同一套 SettingField schema + 表单控件 + secret 掩码语义），
 * 区别：无测试/用量（那是插件生态的 testSources/quota 机制）。
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Plus, X } from "lucide-react";
import { api, type SettingField, type SkillInfo } from "@/api/client";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { CollapsibleCard, SettingFieldInput, type SettingValue } from "./PluginSettingsField";

const EMPTY_SCHEMA: SettingField[] = [];

/** 分组名 = 完整 key 去掉最后一段（点分前缀） */
function groupOf(key: string): string {
  const i = key.lastIndexOf(".");
  return i > 0 ? key.slice(0, i) : key;
}

function groupTitle(group: string, fields: SettingField[]): string {
  const named = fields.find((f) => f.label);
  if (named?.label) return named.label;
  const seg = group.split(".").pop() ?? group;
  return seg.charAt(0).toUpperCase() + seg.slice(1);
}

export function SkillSettingsDialog({
  skill,
  open,
  onClose,
}: {
  skill: SkillInfo;
  open: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [values, setValues] = useState<Record<string, SettingValue>>({});
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  /** 折叠状态：组名 → 是否展开（默认全展开） */
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  /** 待清除的 secret 字段（提交时带 __clear.<key> 标记） */
  const [clears, setClears] = useState<Set<string>>(new Set());

  const schema = skill.settingsSchema ?? EMPTY_SCHEMA;

  // 非 group 字段按点分前缀分组（平铺渲染）
  const groups = useMemo(() => {
    const map = new Map<string, SettingField[]>();
    for (const field of schema) {
      if (field.type === "group") continue;
      const g = groupOf(field.key);
      if (!map.has(g)) map.set(g, []);
      map.get(g)!.push(field);
    }
    return [...map.entries()];
  }, [schema]);

  // group 类型字段（可增删实例的自定义源等）
  const groupFields = useMemo(() => schema.filter((f) => f.type === "group"), [schema]);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const res = await api.getSkillSettings(skill.id);
      setValues(res.settings);
      setClears(new Set());
    } catch (err) {
      toast.error(t("settings.skills.settingsLoadFailed", { error: String(err) }));
      onClose();
    } finally {
      setLoading(false);
    }
  }, [skill.id, onClose, t]);

  useEffect(() => {
    if (open) void load();
  }, [open, load]);

  const setField = (key: string, v: SettingValue) => {
    setValues((prev) => ({ ...prev, [key]: v }));
    if (v !== "" && clears.has(key)) {
      setClears((prev) => {
        const next = new Set(prev);
        next.delete(key);
        return next;
      });
    }
  };

  /** 切换某个 secret 字段的待清除状态（点 × 清除密钥，保存时生效） */
  const toggleClear = (key: string) => {
    setClears((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const toggleCollapsed = (key: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  /** group 字段当前实例数（values 中 customN.* 的最大 N） */
  const groupInstances = (field: SettingField) => {
    const prefix = field.key.replace("*", "");
    let max = 0;
    for (const key of Object.keys(values)) {
      if (!key.startsWith(prefix)) continue;
      const num = Number(key.slice(prefix.length).split(".")[0]);
      if (Number.isInteger(num) && num > max) max = num;
    }
    return max;
  };

  const addGroupInstance = (field: SettingField) => {
    const prefix = field.key.replace("*", "");
    const n = groupInstances(field) + 1;
    setValues((prev) => {
      const next = { ...prev };
      for (const sub of field.fields ?? []) {
        next[`${prefix}${n}.${sub.key}`] =
          sub.default ?? (sub.type === "boolean" ? false : sub.type === "number" ? 0 : "");
      }
      return next;
    });
  };

  const removeGroupInstance = (field: SettingField, n: number) => {
    const prefix = field.key.replace("*", "");
    setValues((prev) => {
      const next = { ...prev };
      for (const sub of field.fields ?? []) delete next[`${prefix}${n}.${sub.key}`];
      return next;
    });
  };

  const save = async () => {
    setSaving(true);
    try {
      const payload = { ...values };
      for (const k of clears) payload[`__clear.${k}`] = true;
      await api.saveSkillSettings(skill.id, payload);
      toast.success(t("settings.skills.settingsSaved"));
      onClose();
    } catch (err) {
      toast.error(t("settings.skills.settingsSaveFailed", { error: String(err) }));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("settings.skills.settingsTitle", { name: skill.name })}</DialogTitle>
          <DialogDescription>{t("settings.skills.settingsDesc")}</DialogDescription>
        </DialogHeader>

        {loading ? (
          <p className="text-sm text-muted-foreground">{t("settings.skills.settingsLoading")}</p>
        ) : (
          <div className="space-y-4">
            {groups.length === 0 && groupFields.length === 0 && (
              <p className="text-sm text-muted-foreground">{t("settings.skills.settingsEmpty")}</p>
            )}

            {/* 平铺分组：按点分前缀分组渲染 */}
            {groups.map(([group, fields]) => {
              const open = !collapsed.has(group);
              return (
                <CollapsibleCard
                  key={group}
                  title={groupTitle(group, fields)}
                  isOpen={open}
                  onToggle={() => toggleCollapsed(group)}
                >
                  <div className="space-y-3">
                    {fields.map((f) => (
                      <div key={f.key}>
                        <SettingFieldInput
                          field={f}
                          value={values[f.key]}
                          onChange={(v) => setField(f.key, v)}
                          cleared={clears.has(f.key)}
                          onToggleClear={() => toggleClear(f.key)}
                        />
                      </div>
                    ))}
                  </div>
                </CollapsibleCard>
              );
            })}

            {/* group 类型字段：按实例数展开为卡片组，可增删 */}
            {groupFields.map((gf) => {
              const n = groupInstances(gf);
              return (
                <CollapsibleCard
                  key={gf.key}
                  title={gf.label || gf.key.replace("*", "")}
                  isOpen={!collapsed.has(gf.key)}
                  onToggle={() => toggleCollapsed(gf.key)}
                  actions={
                    <Button type="button" size="sm" variant="ghost" onClick={() => addGroupInstance(gf)}>
                      <Plus size={14} />
                    </Button>
                  }
                >
                  <div className="space-y-4">
                    {Array.from({ length: n }, (_, i) => i + 1).map((num) => (
                      <div key={num} className="space-y-3 rounded-lg border p-3">
                        <div className="flex items-center justify-between">
                          <span className="text-xs font-medium text-muted-foreground">
                            {gf.label || gf.key.replace("*", "")} {num}
                          </span>
                          <button
                            type="button"
                            onClick={() => removeGroupInstance(gf, num)}
                            className="rounded p-0.5 text-muted-foreground hover:text-destructive"
                            aria-label={t("settings.skills.removeInstance")}
                          >
                            <X size={14} />
                          </button>
                        </div>
                        {(gf.fields ?? []).map((sub) => {
                          const key = `${gf.key.replace("*", "")}${num}.${sub.key}`;
                          return (
                            <div key={key}>
                              <SettingFieldInput
                                field={sub}
                                value={values[key]}
                                onChange={(v) => setField(key, v)}
                                cleared={clears.has(key)}
                                onToggleClear={() => toggleClear(key)}
                              />
                            </div>
                          );
                        })}
                      </div>
                    ))}
                    {n === 0 && (
                      <p className="text-xs text-muted-foreground">{t("settings.skills.settingsEmpty")}</p>
                    )}
                  </div>
                </CollapsibleCard>
              );
            })}
          </div>
        )}

        <DialogFooter>
          <Button onClick={() => void save()} disabled={loading || saving}>
            {t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
