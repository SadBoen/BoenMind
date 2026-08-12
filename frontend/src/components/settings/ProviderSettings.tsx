/**
 * 模型提供商设置（参照 pi-web 的 ModelsConfig 设计）：
 * - 提供商卡片列表：品牌图标 + 状态 + 模型标签 + 默认/编辑/删除
 * - 添加提供商：分组卡片选择器（热门推荐 / 更多 / 本地与兼容），选中即带入预设
 * - 编辑表单：品牌图标随类型联动，切换类型自动填充官方端点与默认模型
 *
 * 预设数据（端点/默认模型/分组/图标）在 lib/provider-presets.tsx 单一数据源。
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/app-store";
import { Plus, Pencil, Trash2, Star, Server, KeyRound } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { toast } from "sonner";
import type { ProviderConfig, ProviderKind } from "@/api/client";
import { KIND_PRESETS } from "@/lib/provider-presets";
import { ProviderIcon } from "./provider-icons";
import { ProviderPicker } from "./ProviderPicker";
import { ProviderFormDialog } from "./ProviderFormDialog";

function emptyProvider(kind: ProviderKind = "openai"): ProviderConfig {
  const preset = KIND_PRESETS[kind];
  return {
    id: `p-${Date.now().toString(36)}`,
    name: "",
    kind,
    base_url: preset.base_url,
    api_key: "",
    models: preset.models,
    default_model: preset.models[0] ?? "",
  };
}

export function ProviderSettings() {
  const { t } = useTranslation();
  const config = useAppStore((s) => s.config);
  const saveConfig = useAppStore((s) => s.saveConfig);

  const [editing, setEditing] = useState<ProviderConfig | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);

  const persist = async (providers: ProviderConfig[], defaultProvider?: string) => {
    if (!config) return;
    try {
      await saveConfig({ ...config, providers, default_provider: defaultProvider });
      toast.success(t("settings.providers.saved"));
    } catch (err) {
      toast.error(t("settings.providers.saveFailed", { error: String(err) }));
    }
  };

  const handleDelete = async (id: string) => {
    if (!config) return;
    const providers = config.providers.filter((p) => p.id !== id);
    await persist(providers, providers[0]?.id);
  };

  const handleDefault = async (id: string) => {
    if (!config) return;
    await persist(config.providers, id);
  };

  /** 选择器选中某类型 → 关闭选择器，打开带预设的表单 */
  const startAddFromKind = (kind: ProviderKind) => {
    setPickerOpen(false);
    setEditing({ ...emptyProvider(kind), name: t(`settings.providers.kinds.${kind}`) });
    setIsNew(true);
  };

  if (!config) {
    return <p className="text-sm text-muted-foreground">{t("settings.providers.loadingConfig")}</p>;
  }

  return (
    <section className="space-y-5">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold">{t("settings.providers.title")}</h2>
          <p className="text-sm text-muted-foreground">{t("settings.providers.desc")}</p>
        </div>
        <Button onClick={() => setPickerOpen(true)}>
          <Plus size={16} className="mr-1" />
          {t("settings.providers.add")}
        </Button>
      </div>

      {config.providers.length === 0 && (
        <div className="rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground">
          {t("settings.providers.empty")}
        </div>
      )}

      <div className="space-y-3">
        {config.providers.map((p) => {
          const isDefault = p.id === config.default_provider;
          const kindLabel = t(`settings.providers.kinds.${p.kind}`);
          return (
            <div
              key={p.id}
              className="flex items-center gap-3 rounded-xl border bg-card p-4 transition-colors hover:bg-accent/40"
            >
              {/* 品牌图标 */}
              <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-muted/60">
                <ProviderIcon kind={p.kind} size={26} />
              </div>

              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <h3 className="truncate font-semibold">{p.name}</h3>
                  <Badge variant={isDefault ? "default" : "secondary"} className="shrink-0 text-[10px]">
                    {isDefault ? t("settings.providers.default") : kindLabel}
                  </Badge>
                </div>
                <p className="mt-0.5 flex items-center gap-1 truncate text-xs text-muted-foreground">
                  <Server size={12} className="shrink-0" />
                  {p.base_url || t("settings.providers.officialEndpoint")}
                  {p.api_key && (
                    <span className="ml-1.5 inline-flex shrink-0 items-center gap-0.5">
                      <KeyRound size={11} /> {t("settings.providers.keyConfigured")}
                    </span>
                  )}
                </p>
                {p.models.length > 0 && (
                  <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
                    {p.models.slice(0, 3).map((m) => (
                      <Badge key={m} variant="outline" className="max-w-40 truncate text-[10px] font-normal">
                        {m}
                      </Badge>
                    ))}
                    {p.models.length > 3 && (
                      <span className="text-[10px] text-muted-foreground">
                        +{p.models.length - 3}
                      </span>
                    )}
                  </div>
                )}
              </div>

              <div className="flex shrink-0 items-center gap-1">
                {!isDefault && (
                  <Button variant="ghost" size="icon" className="h-8 w-8" title={t("settings.providers.setDefault")} onClick={() => void handleDefault(p.id)}>
                    <Star size={15} />
                  </Button>
                )}
                {isDefault && (
                  <span className="flex h-8 w-8 items-center justify-center" title={t("settings.providers.currentDefault")}>
                    <Star size={15} className="fill-amber-400 text-amber-400" />
                  </span>
                )}
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8"
                  title={t("settings.providers.edit")}
                  onClick={() => {
                    setEditing({ ...p });
                    setIsNew(false);
                  }}
                >
                  <Pencil size={15} />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 text-destructive"
                  title={t("settings.providers.delete")}
                  onClick={() => void handleDelete(p.id)}
                >
                  <Trash2 size={15} />
                </Button>
              </div>
            </div>
          );
        })}
      </div>

      <ProviderPicker open={pickerOpen} onOpenChange={setPickerOpen} onPick={startAddFromKind} />

      {editing && (
        <ProviderFormDialog
          provider={editing}
          isNew={isNew}
          onClose={() => setEditing(null)}
          onSave={async (updated) => {
            if (!config) return;
            const exists = config.providers.some((p) => p.id === updated.id);
            const providers = exists
              ? config.providers.map((p) => (p.id === updated.id ? updated : p))
              : [...config.providers, updated];
            const defaultProvider = isNew && !config.default_provider ? updated.id : config.default_provider;
            await persist(providers, defaultProvider);
            setEditing(null);
          }}
        />
      )}
    </section>
  );
}