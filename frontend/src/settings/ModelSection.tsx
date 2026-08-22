import { useEffect, useState } from "react";
import { Row, Select, SettingsForm } from "../components/SettingsForm";
import { rpc } from "../lib/api";
import { uid } from "../lib/format";
import { IconEdit, IconKey, IconPlus, IconServer, IconStar, IconStarFill, IconTrash } from "../lib/icons";
import { fetchProviderPresets } from "../lib/provider-api";
import { applyApiPresets, KIND_LABELS, KIND_PRESETS } from "../lib/provider-presets";
import { useStore } from "../store";
import { toast } from "../lib/toast";
import type { ProviderConfig, ProviderKind } from "../types";
import { ProviderFormDialog } from "./ProviderFormDialog";
import { ProviderIcon } from "./provider-icons";
import { ProviderPicker } from "./ProviderPicker";

function emptyProvider(kind: ProviderKind = "minimax"): ProviderConfig {
  const preset = KIND_PRESETS[kind];
  return {
    id: uid("p"),
    name: "",
    kind,
    base_url: preset.base_url,
    api_key: "",
    models: preset.models,
    default_model: preset.models[0] ?? "",
  };
}

export default function ModelSection() {
  const { state, dispatch } = useStore();
  const [editing, setEditing] = useState<ProviderConfig | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [, setPresetsVersion] = useState(0);

  useEffect(() => {
    let cancelled = false;
    void fetchProviderPresets().then((presets) => {
      if (cancelled) return;
      applyApiPresets(presets);
      setPresetsVersion((v) => v + 1);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const persist = (providers: ProviderConfig[], defaultProvider?: string, defaultModel?: string) => {
    const nextDefault = defaultProvider ?? state.settings.defaultProvider;
    const nextModel = defaultModel ?? state.settings.defaultModel;
    dispatch({
      type: "patch-settings",
      patch: { providers, defaultProvider: nextDefault, defaultModel: nextModel },
    });
  };

  const startAddFromKind = (kind: ProviderKind) => {
    setPickerOpen(false);
    setEditing({ ...emptyProvider(kind), name: KIND_LABELS[kind] });
    setIsNew(true);
  };

  const handleSave = async (p: ProviderConfig) => {
    // API Key 走后端 credentials（{KIND}_API_KEY ref；后端同步进 provider 适配器，
    // 值永不出域、不进 localStorage）。保存失败则中止（密钥不丢）。
    if (p.api_key?.trim()) {
      const r = await rpc("credentials.set", { ref: `${p.kind.toUpperCase()}_API_KEY`, value: p.api_key.trim() });
      if (!r.ok) {
        toast.error(`API Key 保存到后端失败：${r.error.message}`);
        return;
      }
    }
    // 本地只存脱敏后的提供商配置（api_key 恒空）。
    const saved = { ...p, api_key: "" };
    const list = isNew ? [...state.settings.providers, saved] : state.settings.providers.map((x) => (x.id === saved.id ? saved : x));
    const nextDefaultProvider = state.settings.defaultProvider || saved.id;
    const nextDefaultModel =
      nextDefaultProvider === saved.id ? (saved.default_model ?? saved.models[0] ?? state.settings.defaultModel) : state.settings.defaultModel;
    persist(list, nextDefaultProvider, nextDefaultModel);
    toast.success("已保存提供商");
    setEditing(null);
  };

  const handleDelete = (id: string) => {
    dispatch({
      type: "ask-confirm",
      confirm: {
        title: "删除提供商",
        body: "确认删除这个提供商？",
        confirmLabel: "删除",
        danger: true,
        onConfirm: () => {
          const providers = state.settings.providers.filter((p) => p.id !== id);
          persist(providers, providers[0]?.id ?? "", providers[0]?.default_model ?? providers[0]?.models[0] ?? state.settings.defaultModel);
          toast.success("已删除");
        },
      },
    });
  };

  const handleDefault = (id: string) => {
    const p = state.settings.providers.find((x) => x.id === id);
    persist(state.settings.providers, id, p?.default_model ?? p?.models[0] ?? state.settings.defaultModel);
  };

  return (
    <div className="model-section">
      <SettingsForm>
        <Row label="默认推理档位">
          <Select
            value={state.settings.defaultReasoning}
            onChange={(v) => dispatch({ type: "patch-settings", patch: { defaultReasoning: v as typeof state.settings.defaultReasoning } })}
            options={[
              { value: "off", label: "off" },
              { value: "low", label: "low" },
              { value: "medium", label: "medium" },
              { value: "high", label: "high" },
            ]}
          />
        </Row>
      </SettingsForm>

      <div className="provider-head">
        <div>
          <h2>模型提供商</h2>
          <p>添加云端或本地兼容端点。默认提供商的模型会出现在输入框旁。</p>
        </div>
        <button type="button" className="btn-solid" onClick={() => setPickerOpen(true)}>
          <IconPlus /> 添加
        </button>
      </div>

      {state.settings.providers.length === 0 && <div className="provider-empty">还没有提供商。点右上角添加 MiniMax、DeepSeek 或自定义端点。</div>}

      <div className="provider-list">
        {state.settings.providers.map((p) => {
          const isDefault = p.id === state.settings.defaultProvider;
          return (
            <div key={p.id} className="provider-card">
              <div className="provider-mark is-lg">
                <ProviderIcon kind={p.kind} size={26} />
              </div>
              <div className="provider-meta">
                <div className="provider-title">
                  <h3>{p.name}</h3>
                  <span className={`provider-badge${isDefault ? " is-on" : ""}`}>{isDefault ? "默认" : KIND_LABELS[p.kind]}</span>
                </div>
                <p className="provider-url">
                  <IconServer />
                  {p.base_url || "官方端点"}
                  {p.api_key ? (
                    <span>
                      <IconKey /> 已配置密钥
                    </span>
                  ) : null}
                </p>
                {p.models.length > 0 && (
                  <div className="provider-models">
                    {p.models.slice(0, 3).map((m) => (
                      <span key={m} className="provider-chip">
                        {m}
                      </span>
                    ))}
                    {p.models.length > 3 && <span className="muted">+{p.models.length - 3}</span>}
                  </div>
                )}
              </div>
              <div className="provider-actions">
                <button type="button" className="icon-btn" title={isDefault ? "当前默认" : "设为默认"} onClick={() => handleDefault(p.id)}>
                  {isDefault ? <IconStarFill className="is-star-on" /> : <IconStar />}
                </button>
                <button
                  type="button"
                  className="icon-btn"
                  title="编辑"
                  onClick={() => {
                    setEditing({ ...p });
                    setIsNew(false);
                  }}
                >
                  <IconEdit />
                </button>
                <button type="button" className="icon-btn" title="删除" onClick={() => handleDelete(p.id)}>
                  <IconTrash />
                </button>
              </div>
            </div>
          );
        })}
      </div>

      <ProviderPicker open={pickerOpen} onClose={() => setPickerOpen(false)} onPick={startAddFromKind} />
      {editing && (
        <ProviderFormDialog
          provider={editing}
          isNew={isNew}
          onClose={() => setEditing(null)}
          onSave={handleSave}
        />
      )}
    </div>
  );
}
