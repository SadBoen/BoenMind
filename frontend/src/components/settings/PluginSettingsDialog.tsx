/**
 * 插件设置对话框：按插件 manifest 声明的 settings schema 动态渲染。
 * 布局：用量进度条区块 + 折叠卡片（搜索设置 / 每个供应商 / 每个自定义源），
 * 卡片带启停开关与「测试」按钮（连通性探测，测的是表单当前值并实时刷新用量）。
 * 字段类型：string / secret（已配置显示掩码提示 + 清除按钮）/ boolean /
 * number（空值保存时回退默认）/ select / group（可增删实例）。
 * 保存后对话框保持打开，可连续调整；「恢复默认」一键回到 schema 默认值。
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import type { SettingValue } from "./PluginSettingsField";
import { CollapsibleCard, SettingFieldInput } from "./PluginSettingsField";
import { useTranslation } from "react-i18next";
import { Loader2, PlugZap, Plus, RotateCcw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { toast } from "sonner";
import { api, type PluginInfo, type QuotaInfo, type SettingField } from "@/api/client";

/** schema 为空的兜底（模块级常量：避免每次 render 新建引用导致 useMemo 失效） */
const EMPTY_SCHEMA: SettingField[] = [];

interface Props {
  plugin: PluginInfo;
  open: boolean;
  onClose: () => void;
}

/** 该源是否有测试模板（manifest testSources 精确匹配，或 `x*` 通配键前缀匹配），
 *  并给出展示名。无模板的源不渲染「测试」按钮——新增源只需在 extension.json 声明，
 *  前端零改动。 */
function testableSource(plugin: PluginInfo, sourceKey: string): { label: string; testable: boolean } {
  const sources = plugin.testSources;
  if (!sources) return { label: sourceKey, testable: false };
  const exact = sources[sourceKey];
  if (exact) return { label: exact.label ?? sourceKey, testable: true };
  for (const [wild, decl] of Object.entries(sources)) {
    if (wild.endsWith("*") && sourceKey.startsWith(wild.slice(0, -1))) {
      return { label: decl.label ?? sourceKey, testable: true };
    }
  }
  return { label: sourceKey, testable: false };
}

/** 组 = key 去掉最后一段（如 sources.jina.apiKey → sources.jina） */
function groupOf(key: string): string {
  const idx = key.lastIndexOf(".");
  return idx > 0 ? key.slice(0, idx) : "__root";
}

/** 组标题：manifest 声明的 groupLabel（组内任一字段）→ 原始 key */
function groupTitle(group: string, fields: SettingField[]): string {
  return fields.find((f) => f.groupLabel)?.groupLabel || group;
}

/** 进度条颜色：>80% 红、>50% 琥珀、否则主题色 */
function barColor(pct: number): string {
  if (pct >= 80) return "bg-red-500";
  if (pct >= 50) return "bg-amber-500";
  return "bg-primary";
}

/** 千分位格式化（10000000 → 10,000,000） */
function fmt(n: number): string {
  return n.toLocaleString();
}

export function PluginSettingsDialog({ plugin, open, onClose }: Props) {
  const { t } = useTranslation();
  const [values, setValues] = useState<Record<string, SettingValue>>({});
  const [quota, setQuota] = useState<Record<string, QuotaInfo> | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState<string | null>(null);
  /** 折叠状态：卡片标题 → 是否展开（默认全展开） */
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  /** 待清除的 secret 字段（提交时带 __clear.<key> 标记） */
  const [clears, setClears] = useState<Set<string>>(new Set());

  const schema = plugin.settingsSchema ?? EMPTY_SCHEMA;

  // 所有非 group 字段按点分前缀分组（组 = 完整 key 去掉最后一段）。
  // 组是否渲染为「卡片」（折叠 + 启停开关 + 测试按钮）由 manifest 决定：
  // 组内含 `.enabled` 布尔字段 → 卡片；否则为固定平铺区（如搜索设置）。
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
      const res = await api.getPluginSettings(plugin.id);
      setValues(res.settings);
      setQuota(res.quota ?? null);
      setClears(new Set());
    } catch (err) {
      toast.error(t("settings.plugins.settingsLoadFailed", { error: String(err) }));
      onClose();
    } finally {
      setLoading(false);
    }
  }, [plugin.id, onClose, t]);

  useEffect(() => {
    if (open) void load();
  }, [open, load]);

  const setField = (key: string, v: SettingValue) => {
    setValues((prev) => ({ ...prev, [key]: v }));
    // 用户输入了新值 → 取消该字段的待清除标记
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

  /** 当前表单值 + 待清除标记（测试与保存共用的提交载荷） */
  const buildPayload = () => {
    const payload = { ...values };
    for (const k of clears) payload[`__clear.${k}`] = true;
    return payload;
  };

  /** 测试源连通性：测的是表单当前值（未保存的修改立即生效），成功后刷新用量 */
  const testSource = async (source: string, display: string) => {
    setTesting(source);
    try {
      const res = await api.testPluginSource(plugin.id, source, buildPayload());
      if (res.quota) setQuota(res.quota);
      if (res.ok) {
        toast.success(`${display} ✓ · ${res.latencyMs}ms · ${res.detail}`);
      } else {
        toast.error(`${display} ✗ · ${res.latencyMs}ms · ${res.detail}`);
      }
    } catch (err) {
      toast.error(`${display} ✗ · ${String(err)}`);
    } finally {
      setTesting(null);
    }
  };

  /** 计算 group 字段当前实例数（从 values 中匹配 customN.* 的最大 N）。
   *  无 manifest 默认下限：删除实例后卡片数可真正缩下去，不会原地复活。 */
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
        next[`${prefix}${n}.${sub.key}`] = sub.default ?? (sub.type === "boolean" ? false : sub.type === "number" ? 0 : "");
      }
      return next;
    });
  };

  const removeGroupInstance = (field: SettingField, n: number) => {
    const prefix = field.key.replace("*", "");
    const max = groupInstances(field);
    setValues((prev) => {
      const next = { ...prev };
      for (let i = n + 1; i <= max; i++) {
        for (const sub of field.fields ?? []) {
          const from = `${prefix}${i}.${sub.key}`;
          const to = `${prefix}${i - 1}.${sub.key}`;
          if (from in next) {
            next[to] = next[from];
            delete next[from];
          }
        }
      }
      for (const sub of field.fields ?? []) delete next[`${prefix}${max}.${sub.key}`];
      return next;
    });
  };

  /** 用量条展示名：`sources.<key>` 组的 manifest 标题；custom: 前缀剥离；否则原始 key */
  const quotaDisplayName = (key: string): string => {
    if (key.startsWith("custom:")) return key.slice("custom:".length);
    const fields = groups.find(([g]) => g === `sources.${key}`)?.[1];
    return fields ? groupTitle(`sources.${key}`, fields) : key;
  };

  /** 恢复默认：普通字段提交 schema 默认值，secret 字段走清除标记 */
  const resetToDefault = () => {
    const next: Record<string, SettingValue> = {};
    const nextClears = new Set<string>();
    const apply = (key: string, f: SettingField) => {
      if (f.type === "secret") {
        nextClears.add(key);
      } else {
        next[key] = (f.default ?? (f.type === "boolean" ? false : f.type === "number" ? 0 : "")) as SettingValue;
      }
    };
    for (const f of schema) {
      if (f.type === "group") {
        const prefix = f.key.replace("*", "");
        // 恢复默认：至少补回 manifest 默认实例数（删到 0 张时一并重建）
        const max = Math.max(groupInstances(f), f.instances ?? 2);
        for (let n = 1; n <= max; n++) {
          for (const sub of f.fields ?? []) apply(`${prefix}${n}.${sub.key}`, sub);
        }
      } else {
        apply(f.key, f);
      }
    }
    setValues(next);
    setClears(nextClears);
    toast.success(t("settings.plugins.resetDone"));
  };

  const confirmReset = () => {
    toast(t("settings.plugins.resetConfirm"), {
      action: { label: t("settings.plugins.resetDo"), onClick: () => resetToDefault() },
      duration: 8000,
    });
  };

  const save = async () => {
    setSaving(true);
    try {
      const res = await api.putPluginSettings(plugin.id, buildPayload());
      setValues(res.settings);
      setClears(new Set());
      // 保持对话框打开，方便连续调整多个源；用量由测试/插件写入，保存不改
      toast.success(t("settings.plugins.settingsSaved", { name: plugin.name }), {
        description: t("settings.plugins.settingsSavedHint"),
      });
    } catch (err) {
      toast.error(t("settings.plugins.settingsSaveFailed", { error: String(err) }));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      {/* 配置项多（用量 + 多源 + 自定义源），用宽对话框；inline style 覆盖默认 sm:max-w-sm */}
      <DialogContent
        className="max-h-[85vh] overflow-y-auto"
        style={{ width: "44rem", maxWidth: "calc(100vw - 2rem)" }}
      >
        <DialogHeader>
          <DialogTitle>{t("settings.plugins.settingsTitle", { name: plugin.name })}</DialogTitle>
          <DialogDescription>{t("settings.plugins.settingsDesc")}</DialogDescription>
        </DialogHeader>

        {loading || schema.length === 0 ? (
          <p className="py-6 text-center text-sm text-muted-foreground">
            {schema.length === 0 ? t("settings.plugins.noSettings") : t("common.loading")}
          </p>
        ) : (
          <div className="space-y-4">
            {/* ── 用量进度条区块 ── */}
            {quota && Object.keys(quota).length > 0 && (
              <div className="rounded-xl border p-4">
                <h4 className="mb-3 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {t("settings.plugins.quotaTitle")}
                </h4>
                <div className="space-y-2.5">
                  {Object.entries(quota).map(([key, q]) => {
                    const displayName = quotaDisplayName(key);
                    // tokens 源无法精确统计：不显示会误导的 0%，只展示额度声明
                    const isTokens = q.unit === "tokens";
                    const pct = !isTokens && q.total > 0 ? Math.min(100, Math.round((q.used / q.total) * 100)) : 0;
                    return (
                      <div key={key}>
                        <div className="mb-1 flex items-baseline justify-between gap-2 text-xs">
                          <span className="font-medium">{displayName}</span>
                          <span className="text-muted-foreground">
                            {isTokens
                              ? `${fmt(q.total)} ${q.unit}（${t("settings.plugins.quotaTokensUnknown")}）`
                              : `${fmt(q.used)}/${fmt(q.total)} ${q.unit}（${pct}%）`}
                            {q.callsToday ? ` · ${t("settings.plugins.quotaToday", { n: q.callsToday })}` : ""}
                            {q.exhaustedAt ? ` · ${t("settings.plugins.quotaExhausted")}` : ""}
                          </span>
                        </div>
                        <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
                          <div
                            className={`h-full rounded-full transition-all ${isTokens ? "bg-muted-foreground/25" : barColor(pct)}`}
                            style={{ width: isTokens ? "100%" : `${pct}%` }}
                          />
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}

            {/* ── 分组（按 manifest 展开）：组内含 `.enabled` 布尔字段 → 折叠卡片
                 （如各搜索源：启停开关 + 测试按钮）；否则固定平铺区（如搜索设置） ── */}
            {groups.map(([group, fields]) => {
              const enabledField = fields.find(
                (f) => f.type === "boolean" && f.key.endsWith(".enabled")
              );
              const title = groupTitle(group, fields);
              if (!enabledField) {
                return (
                  <div key={group} className="space-y-3">
                    <h4 className="text-sm font-semibold">{title}</h4>
                    {fields.map((field) => (
                      <SettingFieldInput
                        key={field.key}
                        field={field}
                        value={values[field.key]}
                        onChange={(v) => setField(field.key, v)}
                        disabled={saving}
                        cleared={clears.has(field.key)}
                        onToggleClear={() => toggleClear(field.key)}
                      />
                    ))}
                  </div>
                );
              }
              // 卡片组：源 id = 组最后一段（sources.jina → jina），
              // 测试按钮按 testSources 精确/通配匹配
              const sourceKey = group.slice(group.lastIndexOf(".") + 1);
              const { label: display, testable } = testableSource(plugin, sourceKey);
              const enabled = Boolean(values[enabledField.key]);
              return (
                <CollapsibleCard
                  key={group}
                  title={title}
                  isOpen={!collapsed.has(group)}
                  onToggle={() => toggleCollapsed(group)}
                  actions={
                    <>
                      <Switch
                        checked={enabled}
                        onCheckedChange={(v) => setField(enabledField.key, v)}
                        disabled={saving}
                      />
                      {testable && (
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-7 gap-1 px-2 text-xs"
                          onClick={() => void testSource(sourceKey, display)}
                          disabled={testing !== null || saving}
                        >
                          {testing === sourceKey ? (
                            <Loader2 size={12} className="animate-spin" />
                          ) : (
                            <PlugZap size={12} />
                          )}
                          {t("settings.plugins.test")}
                        </Button>
                      )}
                    </>
                  }
                >
                  <div className="space-y-3">
                    {fields
                      .filter((f) => f !== enabledField)
                      .map((field) => (
                        <SettingFieldInput
                          key={field.key}
                          field={field}
                          value={values[field.key]}
                          onChange={(v) => setField(field.key, v)}
                          disabled={saving}
                          cleared={clears.has(field.key)}
                          onToggleClear={() => toggleClear(field.key)}
                        />
                      ))}
                  </div>
                </CollapsibleCard>
              );
            })}

            {/* ── 自定义源（group 类型，卡片列表 + 添加） ── */}
            {groupFields.map((field) => {
              const prefix = field.key.replace("*", "");
              const max = groupInstances(field);
              return (
                <div key={field.key} className="space-y-3">
                  <div>
                    <h4 className="text-sm font-semibold">{field.label}</h4>
                    {field.description && (
                      <p className="text-xs text-muted-foreground">{field.description}</p>
                    )}
                  </div>
                  {Array.from({ length: max }, (_, i) => i + 1).map((n) => {
                    const cardKey = `${prefix}${n}`;
                    const isOpen = !collapsed.has(cardKey);
                    const name = String(values[`${prefix}${n}.name`] ?? "");
                    const { label: instanceLabel, testable } = testableSource(plugin, cardKey);
                    const enabledField = field.fields?.find((f) => f.key === "enabled");
                    const enabled = Boolean(enabledField && values[`${prefix}${n}.enabled`]);
                    return (
                      <CollapsibleCard
                        key={n}
                        title={name || t("settings.plugins.groupInstance", { n })}
                        isOpen={isOpen}
                        onToggle={() => toggleCollapsed(cardKey)}
                        actions={
                          <>
                            {enabledField && (
                              <Switch
                                checked={enabled}
                                onCheckedChange={(v) => setField(`${prefix}${n}.enabled`, v)}
                                disabled={saving}
                              />
                            )}
                            {testable && (
                              <Button
                                variant="outline"
                                size="sm"
                                className="h-7 gap-1 px-2 text-xs"
                                onClick={() =>
                                  void testSource(cardKey, instanceLabel || t("settings.plugins.groupInstance", { n }))
                                }
                                disabled={testing !== null || saving}
                              >
                                {testing === cardKey ? (
                                  <Loader2 size={12} className="animate-spin" />
                                ) : (
                                  <PlugZap size={12} />
                                )}
                                {t("settings.plugins.test")}
                              </Button>
                            )}
                            {/* 任意一张卡都可删除（含最后一张）；删光后靠「添加源」重建 */}
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-7 px-2 text-xs text-muted-foreground hover:text-destructive"
                              onClick={() => removeGroupInstance(field, n)}
                              disabled={saving}
                            >
                              <Trash2 size={12} />
                            </Button>
                          </>
                        }
                      >
                        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                          {(field.fields ?? [])
                            .filter((f) => f.key !== "enabled")
                            .map((sub) => (
                              <div key={sub.key} className={sub.type === "boolean" ? "sm:col-span-2" : ""}>
                                <SettingFieldInput
                                  field={{ ...sub, key: `${prefix}${n}.${sub.key}` }}
                                  value={values[`${prefix}${n}.${sub.key}`]}
                                  onChange={(v) => setField(`${prefix}${n}.${sub.key}`, v)}
                                  disabled={saving}
                                  cleared={clears.has(`${prefix}${n}.${sub.key}`)}
                                  onToggleClear={() => toggleClear(`${prefix}${n}.${sub.key}`)}
                                />
                              </div>
                            ))}
                        </div>
                      </CollapsibleCard>
                    );
                  })}
                  <Button variant="outline" size="sm" onClick={() => addGroupInstance(field)} disabled={saving}>
                    <Plus size={13} className="mr-1" />
                    {t("settings.plugins.groupAdd")}
                  </Button>
                </div>
              );
            })}
          </div>
        )}

        <DialogFooter className="justify-between">
          <Button variant="outline" onClick={confirmReset} disabled={loading || saving}>
            <RotateCcw size={13} className="mr-1" />
            {t("settings.plugins.reset")}
          </Button>
          <div className="flex gap-2">
            <Button variant="outline" onClick={onClose} disabled={saving}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => void save()} disabled={loading || saving}>
              {saving ? t("common.saving") : t("common.save")}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}