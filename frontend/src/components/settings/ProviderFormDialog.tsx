/**
 * 提供商表单对话框：类型切换联动预设、拉取模型、测试连接、校验与保存。
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, PlugZap, RefreshCw } from "lucide-react";
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
import { toast } from "sonner";
import { api } from "@/api/client";
import type { ProviderConfig, ProviderKind, ProviderShape } from "@/api/client";
import { KIND_PRESETS, KIND_VALUES } from "@/lib/provider-presets";
import { ProviderIcon } from "./provider-icons";
import { ModelListCombobox } from "./ModelListCombobox";

export function ProviderFormDialog({
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
        shape: form.shape,
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
        shape: form.shape,
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
      // 切换类型时重置协议形状（内置厂商固定 OpenAI 兼容；custom 重新选择）
      shape: undefined,
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
      {/* 黄金比例宽度（页宽 768 × 0.618 ≈ 29.6rem）——2026-08-16 统一 */}
      <DialogContent size="md">
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

          {/* 协议形状（仅自定义厂商）：OpenAI 兼容 / Anthropic 方言 / Gemini 方言。
              内置厂商形状固定（minimax/deepseek = OpenAI 兼容），不显示 */}
          {form.kind === "custom" && (
            <div className="space-y-1.5">
              <Label>
                {t("settings.providers.shapeLabel")}
                <span className="ml-1 text-xs font-normal text-muted-foreground">
                  {t("settings.providers.shapeHint")}
                </span>
              </Label>
              <Select
                value={form.shape ?? "openai-compatible"}
                onValueChange={(v) => setForm({ ...form, shape: v as ProviderShape })}
                itemToStringLabel={(v) => t(`settings.providers.shapes.${v}`)}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(["openai-compatible", "anthropic", "gemini"] as ProviderShape[]).map((s) => (
                    <SelectItem key={s} value={s}>
                      {t(`settings.providers.shapes.${s}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}

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
