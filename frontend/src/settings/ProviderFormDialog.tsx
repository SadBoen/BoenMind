import { useEffect, useState } from "react";
import { rpc } from "../lib/api";
import { IconPlug, IconRefresh } from "../lib/icons";
import { listProviderModels, testProvider } from "../lib/provider-api";
import { KIND_LABELS, KIND_PRESETS, KIND_VALUES, SHAPE_LABELS } from "../lib/provider-presets";
import { toast } from "../lib/toast";
import type { ProviderConfig, ProviderKind, ProviderShape } from "../types";
import { ModelListCombobox } from "./ModelListCombobox";
import { ProviderIcon } from "./provider-icons";

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
  const [form, setForm] = useState<ProviderConfig>(provider);
  const [saving, setSaving] = useState(false);
  const [fetchingModels, setFetchingModels] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; detail: string } | null>(null);
  // 后端是否已存该类型的 Key（credentials.describe 只回 configured，不回值）。
  // 已配置时空 Key 允许保存（保留原 Key 不变）。
  const [keyConfigured, setKeyConfigured] = useState(false);
  useEffect(() => {
    let cancelled = false;
    const ref = `${form.kind.toUpperCase()}_API_KEY`;
    void rpc<{ credentials: Record<string, { configured: boolean }> }>("credentials.describe", { refs: [ref] }).then((r) => {
      if (cancelled || !r.ok) return;
      setKeyConfigured(Boolean(r.value.credentials?.[ref]?.configured));
    });
    return () => {
      cancelled = true;
    };
  }, [form.kind]);

  const fetchModels = async () => {
    setFetchingModels(true);
    setTestResult(null);
    try {
      const models = await listProviderModels({
        kind: form.kind,
        shape: form.shape,
        base_url: form.base_url ?? "",
        api_key: form.api_key ?? "",
      });
      setForm((f) => ({
        ...f,
        models,
        default_model: f.default_model && models.includes(f.default_model) ? f.default_model : (models[0] ?? undefined),
      }));
      toast.success(`已拉取 ${models.length} 个模型`);
    } catch (err) {
      setTestResult({ ok: false, detail: String(err) });
    } finally {
      setFetchingModels(false);
    }
  };

  const runTest = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const res = await testProvider({
        kind: form.kind,
        shape: form.shape,
        base_url: form.base_url ?? "",
        api_key: form.api_key ?? "",
        model: form.default_model ?? form.models[0] ?? "",
        message: "ping",
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
      shape: undefined,
      base_url: preset.base_url ?? "",
      models: preset.models ?? [],
      default_model: (preset.models ?? [])[0] ?? undefined,
    }));
    setTestResult(null);
  };

  const submit = async () => {
    if (!form.name.trim()) {
      toast.error("请填写名称");
      return;
    }
    const needsKey = KIND_PRESETS[form.kind].needsKey ?? true;
    if (needsKey && !form.api_key?.trim() && !keyConfigured) {
      toast.error("请填写 API Key");
      return;
    }
    if (form.models.length === 0) {
      toast.error("至少添加一个模型");
      return;
    }
    setSaving(true);
    try {
      await onSave({
        ...form,
        name: form.name.trim(),
        default_model: form.default_model ?? form.models[0],
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-center" role="dialog" aria-modal="true" aria-labelledby="provider-form-title">
      <div className="dialog-card is-md">
        <div className="dialog-head">
          <span className="provider-mark">
            <ProviderIcon kind={form.kind} size={20} />
          </span>
          <h3 id="provider-form-title">{isNew ? "添加提供商" : "编辑提供商"}</h3>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={onClose}>
            ×
          </button>
        </div>
        <div className="form-stack">
          <div className="form-grid-2">
            <label className="form-field">
              <span>名称</span>
              <input className="field" value={form.name} placeholder="显示名" onChange={(e) => setForm({ ...form, name: e.target.value })} />
            </label>
            <label className="form-field">
              <span>类型</span>
              <select className="field" value={form.kind} onChange={(e) => changeKind(e.target.value as ProviderKind)}>
                {KIND_VALUES.map((k) => (
                  <option key={k} value={k}>
                    {KIND_LABELS[k]}
                  </option>
                ))}
              </select>
            </label>
          </div>
          {form.kind === "custom" && (
            <label className="form-field">
              <span>
                协议形状 <i className="field-hint">自定义厂商需要选择方言</i>
              </span>
              <select
                className="field"
                value={form.shape ?? "openai-compatible"}
                onChange={(e) => setForm({ ...form, shape: e.target.value as ProviderShape })}
              >
                {(Object.keys(SHAPE_LABELS) as ProviderShape[]).map((s) => (
                  <option key={s} value={s}>
                    {SHAPE_LABELS[s]}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label className="form-field">
            <span>
              API 端点 <i className="field-hint">留空则用官方默认</i>
            </span>
            <input
              className="field"
              value={form.base_url ?? ""}
              placeholder="https://api.deepseek.com/v1"
              onChange={(e) => setForm({ ...form, base_url: e.target.value })}
            />
          </label>
          <label className="form-field">
            <span>API Key{keyConfigured ? "（已配置，留空保持不变）" : ""}</span>
            <input
              className="field"
              type="password"
              value={form.api_key ?? ""}
              placeholder={keyConfigured ? "已配置，留空保持不变" : "sk-..."}
              autoComplete="off"
              onChange={(e) => setForm({ ...form, api_key: e.target.value })}
            />
          </label>
          <div className="form-field">
            <button type="button" className="btn-ghost" onClick={() => void runTest()} disabled={testing}>
              <IconPlug /> {testing ? "测试中…" : "测试连接"}
            </button>
            {testResult && <p className={testResult.ok ? "test-ok" : "test-fail"}>{testResult.detail}</p>}
          </div>
          <div className="form-field">
            <span className="form-field-row">
              模型列表
              <button type="button" className="btn-ghost is-tiny" onClick={() => void fetchModels()} disabled={fetchingModels}>
                <IconRefresh /> {fetchingModels ? "拉取中…" : "拉取模型"}
              </button>
            </span>
            <ModelListCombobox
              models={form.models}
              defaultModel={form.default_model}
              onModelsChange={(models) => setForm((f) => ({ ...f, models }))}
              onDefaultChange={(model) => setForm((f) => ({ ...f, default_model: model }))}
            />
          </div>
        </div>
        <div className="modal-actions">
          <button type="button" className="btn-ghost" onClick={onClose}>
            取消
          </button>
          <button type="button" className="btn-solid" onClick={() => void submit()} disabled={saving}>
            {saving ? "保存中…" : "保存"}
          </button>
        </div>
      </div>
    </div>
  );
}
