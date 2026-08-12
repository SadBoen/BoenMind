/**
 * 模型提供商设置（参照 pi-web 的 ModelsConfig 设计）：
 * - 提供商卡片列表：品牌图标 + 状态 + 模型标签 + 默认/编辑/删除
 * - 添加提供商：分组卡片选择器（热门推荐 / 更多 / 本地与兼容），选中即带入预设
 * - 编辑表单：品牌图标随类型联动，切换类型自动填充官方端点与默认模型
 *
 * 预设数据（端点/默认模型/分组/图标）在 lib/provider-presets.tsx 单一数据源。
 */
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Plus,
  Pencil,
  Trash2,
  Star,
  Server,
  KeyRound,
  Search,
  RefreshCw,
  PlugZap,
  Loader2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Badge } from "@/components/ui/badge";
import { toast } from "sonner";
import { api } from "@/api/client";
import { useAppStore } from "@/stores/app-store";
import type { ProviderConfig, ProviderKind } from "@/api/client";
import { KIND_GROUPS, KIND_PRESETS, KIND_VALUES } from "@/lib/provider-presets";
import { ProviderIcon } from "./provider-icons";
import { ModelListCombobox } from "./ModelListCombobox";

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

// ── 添加提供商选择器（参照 pi-web AddProviderPicker） ─────────────────────────

function ProviderPicker({
  open,
  onOpenChange,
  onPick,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (kind: ProviderKind) => void;
}) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setSearch("");
      setTimeout(() => inputRef.current?.focus(), 30);
    }
  }, [open]);

  const q = search.trim().toLowerCase();
  const matches = (k: ProviderKind) =>
    !q || t(`settings.providers.kinds.${k}`).toLowerCase().includes(q) || k.includes(q);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("settings.providers.pickerTitle")}</DialogTitle>
        </DialogHeader>

        {/* 搜索 */}
        <div className="flex items-center gap-2 rounded-lg border bg-muted/40 px-3">
          <Search size={14} className="shrink-0 text-muted-foreground" />
          <input
            ref={inputRef}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") onOpenChange(false);
            }}
            placeholder={t("settings.providers.pickerSearch")}
            className="h-9 flex-1 border-none bg-transparent text-sm outline-none placeholder:text-muted-foreground"
          />
        </div>

        <div className="max-h-[55vh] overflow-y-auto pr-1">
          {KIND_GROUPS.map(({ group, kinds }) => {
            const visible = kinds.filter(matches);
            if (visible.length === 0) return null;
            return (
              <div key={group} className="mb-4 last:mb-0">
                <div className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
                  {t(`settings.providers.picker.${group}`)}
                </div>
                <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
                  {visible.map((k) => (
                    <button
                      key={k}
                      type="button"
                      onClick={() => onPick(k)}
                      className="flex items-center gap-2.5 rounded-lg border bg-card p-3 text-left transition-colors hover:bg-accent/60"
                    >
                      <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-muted/60">
                        <ProviderIcon kind={k} size={22} />
                      </div>
                      <div className="min-w-0">
                        <div className="truncate text-sm font-medium">
                          {t(`settings.providers.kinds.${k}`)}
                        </div>
                        <div className="truncate text-[10px] text-muted-foreground">
                          {KIND_PRESETS[k].base_url ||
                            t("settings.providers.officialEndpoint")}
                        </div>
                      </div>
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
          {KIND_GROUPS.every(({ kinds }) => kinds.filter(matches).length === 0) && (
            <div className="py-10 text-center text-sm text-muted-foreground">
              {t("settings.providers.pickerEmpty")}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

// ── 提供商表单对话框 ──────────────────────────────────────────────────────────

function ProviderFormDialog({
  provider,
  isNew,
  onClose,
  onSave,
}: {
  provider: ProviderConfig;
  isNew: boolean;
  onClose: () => void;
  onSave: (p: ProviderConfig) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [form, setForm] = useState<ProviderConfig>(provider);
  const [saving, setSaving] = useState(false);
  const [fetchingModels, setFetchingModels] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; detail: string } | null>(null);

  /** 向提供商接口拉取模型列表并替换模型列表 */
  const fetchModels = async () => {
    setFetchingModels(true);
    setTestResult(null);
    try {
      const { models } = await api.listProviderModels({
        kind: form.kind,
        base_url: form.base_url ?? "",
        api_key: form.api_key ?? "",
      });
      setForm((f) => ({
        ...f,
        models,
        default_model:
          f.default_model && models.includes(f.default_model) ? f.default_model : (models[0] ?? undefined),
      }));
      toast.success(t("settings.providers.modelsFetched", { count: models.length }));
    } catch (err) {
      setTestResult({ ok: false, detail: String(err) });
    } finally {
      setFetchingModels(false);
    }
  };

  /** 测试连接：使用预设语句发送真实对话，验证连通与模型回复 */
  const runTest = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const res = await api.testProvider({
        kind: form.kind,
        base_url: form.base_url ?? "",
        api_key: form.api_key ?? "",
        model: form.default_model ?? form.models[0] ?? "",
        message: t("settings.providers.testMessage"),
      });
      setTestResult({ ok: true, detail: res.detail });
    } catch (err) {
      setTestResult({ ok: false, detail: String(err) });
    } finally {
      setTesting(false);
    }
  };

  const changeKind = (kind: ProviderKind) => {
    const preset = KIND_PRESETS[kind];
    setForm((f) => ({
      ...f,
      kind,
      base_url: preset.base_url ?? "",
      models: preset.models ?? [],
      // 切换类型时默认模型跟随新预设，避免残留旧模型名
      default_model: (preset.models ?? [])[0] ?? undefined,
    }));
    setTestResult(null);
  };

  const submit = async () => {
    if (!form.name.trim()) {
      toast.error(t("settings.providers.nameRequired"));
      return;
    }
    // 免 key 名单由预设表声明（新增本地型提供商无需改这里）
    const needsKey = KIND_PRESETS[form.kind].needsKey ?? true;
    if (needsKey && !form.api_key?.trim()) {
      toast.error(t("settings.providers.keyRequired"));
      return;
    }
    const models = form.models;
    if (models.length === 0) {
      toast.error(t("settings.providers.modelsRequired"));
      return;
    }
    setSaving(true);
    try {
      await onSave({
        ...form,
        name: form.name.trim(),
        models,
        default_model: form.default_model ?? models[0],
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2.5">
            {/* 品牌图标随类型联动 */}
            <span className="flex h-8 w-8 items-center justify-center rounded-lg bg-muted/60">
              <ProviderIcon kind={form.kind} size={20} />
            </span>
            {isNew ? t("settings.providers.addDialog") : t("settings.providers.editDialog")}
          </DialogTitle>
        </DialogHeader>
        <div className="grid gap-4 py-2">
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>{t("settings.providers.nameLabel")}</Label>
              <Input
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                placeholder={t("settings.providers.namePlaceholder")}
              />
            </div>
            <div className="space-y-1.5">
              <Label>{t("settings.providers.kindLabel")}</Label>
              <Select
                value={form.kind}
                onValueChange={(v) => changeKind(v as ProviderKind)}
                itemToStringLabel={(v) => t(`settings.providers.kinds.${v}`)}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {KIND_VALUES.map((k) => (
                    <SelectItem key={k} value={k}>
                      {t(`settings.providers.kinds.${k}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="space-y-1.5">
            <Label>
              {t("settings.providers.apiEndpoint")}
              <span className="ml-1 text-xs font-normal text-muted-foreground">
                {t("settings.providers.apiEndpointHint")}
              </span>
            </Label>
            <Input
              value={form.base_url ?? ""}
              onChange={(e) => setForm({ ...form, base_url: e.target.value })}
              placeholder="https://api.deepseek.com/v1"
            />
          </div>

          <div className="space-y-1.5">
            <Label>{t("settings.providers.apiKeyLabel")}</Label>
            <Input
              type="password"
              value={form.api_key ?? ""}
              onChange={(e) => setForm({ ...form, api_key: e.target.value })}
              placeholder="sk-..."
              autoComplete="off"
            />
          </div>

          {/* 测试连接：一键发送预设语句的真实对话 */}
          <div className="space-y-1.5">
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-8"
              onClick={() => void runTest()}
              disabled={testing}
            >
              {testing ? (
                <Loader2 size={14} className="mr-1 animate-spin" />
              ) : (
                <PlugZap size={14} className="mr-1" />
              )}
              {t("settings.providers.testConnection")}
            </Button>
            {testResult && (
              <p
                className={`whitespace-pre-wrap break-all text-xs ${
                  testResult.ok ? "text-emerald-600" : "text-destructive"
                }`}
              >
                {testResult.detail}
              </p>
            )}
          </div>

          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <Label>{t("settings.providers.modelsLabel")}</Label>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-6 shrink-0 px-2 text-xs text-muted-foreground"
                onClick={() => void fetchModels()}
                disabled={fetchingModels}
              >
                {fetchingModels ? (
                  <Loader2 size={12} className="mr-1 animate-spin" />
                ) : (
                  <RefreshCw size={12} className="mr-1" />
                )}
                {t("settings.providers.fetchModels")}
              </Button>
            </div>
            {/* 模型列表 + 默认模型合并下拉：可增、删、改，星标即默认 */}
            <ModelListCombobox
              models={form.models}
              defaultModel={form.default_model}
              onModelsChange={(models) => setForm((f) => ({ ...f, models }))}
              onDefaultChange={(model) => setForm((f) => ({ ...f, default_model: model }))}
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button onClick={() => void submit()} disabled={saving}>
            {saving ? t("common.saving") : t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
