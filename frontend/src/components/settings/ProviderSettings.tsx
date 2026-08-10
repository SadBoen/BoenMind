/**
 * 模型提供商设置：增删改提供商（类型 / 端点 / API Key / 模型列表），设置默认。
 */
import { useState } from "react";
import { Plus, Pencil, Trash2, Star, StarOff, Server, KeyRound } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
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
import { useAppStore } from "@/stores/app-store";
import type { ProviderConfig, ProviderKind } from "@/api/client";

const KINDS: { value: ProviderKind; label: string }[] = [
  { value: "openai", label: "OpenAI 兼容" },
  { value: "minimax", label: "MiniMax（国内）" },
  { value: "deepseek", label: "DeepSeek" },
  { value: "openrouter", label: "OpenRouter" },
  { value: "anthropic", label: "Anthropic" },
  { value: "gemini", label: "Google Gemini" },
  { value: "ollama", label: "Ollama（本地）" },
  { value: "llamacpp", label: "llama.cpp（本地）" },
  { value: "custom", label: "自定义 OpenAI 兼容" },
];

const KIND_PRESETS: Record<ProviderKind, { base_url?: string; models?: string[] }> = {
  openai: { base_url: "", models: ["gpt-4o", "gpt-4o-mini"] },
  minimax: {
    base_url: "https://api.minimaxi.com/v1",
    models: ["MiniMax-M3", "MiniMax-M2.5", "MiniMax-M1", "MiniMax-Text-01"],
  },
  deepseek: { base_url: "https://api.deepseek.com/v1", models: ["deepseek-chat", "deepseek-reasoner"] },
  openrouter: { base_url: "https://openrouter.ai/api/v1", models: [] },
  anthropic: { base_url: "", models: ["claude-sonnet-4-5", "claude-haiku-4-5"] },
  gemini: { base_url: "", models: ["gemini-2.5-pro", "gemini-2.5-flash"] },
  ollama: { base_url: "http://127.0.0.1:11434/v1", models: ["qwen3:8b", "llama3.1:8b"] },
  llamacpp: { base_url: "http://127.0.0.1:8080/v1", models: [] },
  custom: { base_url: "", models: [] },
};

function emptyProvider(): ProviderConfig {
  return {
    id: `p-${Date.now().toString(36)}`,
    name: "",
    kind: "openai",
    base_url: "",
    api_key: "",
    models: [],
    default_model: "",
  };
}

export function ProviderSettings() {
  const config = useAppStore((s) => s.config);
  const saveConfig = useAppStore((s) => s.saveConfig);

  const [editing, setEditing] = useState<ProviderConfig | null>(null);
  const [isNew, setIsNew] = useState(false);

  const persist = async (providers: ProviderConfig[], defaultProvider?: string) => {
    if (!config) return;
    try {
      await saveConfig({ ...config, providers, default_provider: defaultProvider });
      toast.success("配置已保存");
    } catch (err) {
      toast.error(`保存失败: ${String(err)}`);
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

  if (!config) {
    return <p className="text-sm text-muted-foreground">加载配置中…</p>;
  }

  return (
    <section className="space-y-5">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold">模型提供商</h2>
          <p className="text-sm text-muted-foreground">
            配置 LLM 服务商。至少需要一个提供商才能开始对话。
          </p>
        </div>
        <Button
          onClick={() => {
            setEditing(emptyProvider());
            setIsNew(true);
          }}
        >
          <Plus size={16} className="mr-1" />
          添加提供商
        </Button>
      </div>

      {config.providers.length === 0 && (
        <div className="rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground">
          还没有配置提供商。点击「添加提供商」开始配置。
        </div>
      )}

      <div className="space-y-3">
        {config.providers.map((p) => {
          const isDefault = p.id === config.default_provider;
          const kindLabel = KINDS.find((k) => k.value === p.kind)?.label ?? p.kind;
          return (
            <div key={p.id} className="rounded-xl border p-4">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <h3 className="font-semibold">{p.name}</h3>
                    <Badge variant={isDefault ? "default" : "secondary"} className="text-[10px]">
                      {isDefault ? "默认" : kindLabel}
                    </Badge>
                  </div>
                  <p className="mt-0.5 flex items-center gap-1 truncate text-xs text-muted-foreground">
                    <Server size={12} className="shrink-0" />
                    {p.base_url || "官方端点"}
                    {p.api_key && (
                      <span className="ml-2 inline-flex items-center gap-0.5">
                        <KeyRound size={11} /> 已配置 Key
                      </span>
                    )}
                  </p>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  {!isDefault && (
                    <Button variant="ghost" size="icon" className="h-8 w-8" title="设为默认" onClick={() => void handleDefault(p.id)}>
                      <Star size={15} />
                    </Button>
                  )}
                  {isDefault && (
                    <Button variant="ghost" size="icon" className="h-8 w-8 text-amber-500" title="当前默认" disabled>
                      <StarOff size={15} />
                    </Button>
                  )}
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8"
                    title="编辑"
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
                    title="删除"
                    onClick={() => void handleDelete(p.id)}
                  >
                    <Trash2 size={15} />
                  </Button>
                </div>
              </div>
              <div className="mt-2 flex flex-wrap gap-1.5">
                {p.models.length === 0 && (
                  <span className="text-xs text-muted-foreground">未配置模型</span>
                )}
                {p.models.map((m) => (
                  <Badge key={m} variant="outline" className="text-[10px] font-normal">
                    {m}
                  </Badge>
                ))}
              </div>
            </div>
          );
        })}
      </div>

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
  const [form, setForm] = useState<ProviderConfig>(provider);
  const [modelsText, setModelsText] = useState(provider.models.join("\n"));
  const [saving, setSaving] = useState(false);

  const changeKind = (kind: ProviderKind) => {
    const preset = KIND_PRESETS[kind];
    setForm((f) => ({
      ...f,
      kind,
      base_url: preset.base_url ?? "",
      models: preset.models ?? [],
    }));
    setModelsText((preset.models ?? []).join("\n"));
  };

  const submit = async () => {
    if (!form.name.trim()) {
      toast.error("请填写提供商名称");
      return;
    }
    const needsKey = form.kind !== "ollama" && form.kind !== "llamacpp";
    if (needsKey && !form.api_key?.trim()) {
      toast.error("该类型需要 API Key");
      return;
    }
    const models = modelsText
      .split(/[\n,]/)
      .map((m) => m.trim())
      .filter(Boolean);
    if (models.length === 0) {
      toast.error("请至少填写一个模型");
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
          <DialogTitle>{isNew ? "添加提供商" : "编辑提供商"}</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4 py-2">
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>名称</Label>
              <Input
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                placeholder="例如：DeepSeek"
              />
            </div>
            <div className="space-y-1.5">
              <Label>类型</Label>
              <Select value={form.kind} onValueChange={(v) => changeKind(v as ProviderKind)}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {KINDS.map((k) => (
                    <SelectItem key={k.value} value={k.value}>
                      {k.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="space-y-1.5">
            <Label>
              API 端点
              <span className="ml-1 text-xs font-normal text-muted-foreground">
                （留空使用官方端点；Ollama 等本地服务必填）
              </span>
            </Label>
            <Input
              value={form.base_url ?? ""}
              onChange={(e) => setForm({ ...form, base_url: e.target.value })}
              placeholder="https://api.deepseek.com/v1"
            />
          </div>

          <div className="space-y-1.5">
            <Label>API Key</Label>
            <Input
              type="password"
              value={form.api_key ?? ""}
              onChange={(e) => setForm({ ...form, api_key: e.target.value })}
              placeholder="sk-..."
              autoComplete="off"
            />
          </div>

          <div className="space-y-1.5">
            <Label>模型（每行一个，或逗号分隔）</Label>
            <Textarea
              value={modelsText}
              onChange={(e) => setModelsText(e.target.value)}
              rows={4}
              placeholder={"deepseek-chat\ndeepseek-reasoner"}
            />
          </div>

          <div className="space-y-1.5">
            <Label>默认模型</Label>
            <Select
              value={form.default_model ?? ""}
              onValueChange={(v) => setForm({ ...form, default_model: v ?? undefined })}
            >
              <SelectTrigger>
                <SelectValue placeholder="选择默认模型" />
              </SelectTrigger>
              <SelectContent>
                {modelsText
                  .split(/[\n,]/)
                  .map((m) => m.trim())
                  .filter(Boolean)
                  .map((m) => (
                    <SelectItem key={m} value={m}>
                      {m}
                    </SelectItem>
                  ))}
              </SelectContent>
            </Select>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            取消
          </Button>
          <Button onClick={() => void submit()} disabled={saving}>
            {saving ? "保存中…" : "保存"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
