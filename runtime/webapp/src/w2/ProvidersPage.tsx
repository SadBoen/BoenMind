// W2 设置中心 · 模型提供商页(provider 库 CRUD + 连通探针 + 模型清单拉取
// + 设为当前)。后端 = /admin/providers + /admin/model/active(webadmin.rs)。
// 组件映射:表单内模型选择 = 注册表 elements-model-picker(本体选装);
// 对话框/按钮/输入 = shadcn 官方件。
import { useCallback, useEffect, useState } from "react";
import {
  CheckCircle2Icon,
  CheckIcon,
  Loader2Icon,
  PencilIcon,
  PlugZapIcon,
  PlusIcon,
  RefreshCwIcon,
  Trash2Icon,
  XCircleIcon,
} from "lucide-react";
import { api, type ProbeResult, type Provider } from "./api";
import { ModelPicker, type PickableModel } from "@/components/model-picker";
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

type Draft = {
  id?: string;
  name: string;
  baseUrl: string;
  apiKey: string;
  models: string[];
  modelsCommon: string[];
  defaultModel: string;
};

const emptyDraft: Draft = {
  name: "",
  baseUrl: "",
  apiKey: "",
  models: [],
  modelsCommon: [],
  defaultModel: "",
};

export function ProvidersPage() {
  const [providers, setProviders] = useState<Provider[]>([]);
  const [probes, setProbes] = useState<Record<string, ProbeResult>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setProviders((await api.providers.list()).providers);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const flash = (msg: string) => {
    setNotice(msg);
    setError(null);
    setTimeout(() => setNotice(null), 6000);
  };

  const probe = async (p: Provider) => {
    setBusy(`probe:${p.id}`);
    try {
      // 列表行探针不带已存密钥(前端不持有明文);公网网关的 /models 多为
      // 免鉴权或 401 可见——结果仅反映可达性,凭据有效性以真实对话为准
      const r = await api.providers.probe(p.baseUrl);
      setProbes((cur) => ({ ...cur, [p.id]: r }));
    } catch (e) {
      setProbes((cur) => ({
        ...cur,
        [p.id]: { ok: false, latencyMs: 0, error: String(e) },
      }));
    } finally {
      setBusy(null);
    }
  };

  const setActive = async (p: Provider) => {
    setBusy(`active:${p.id}`);
    try {
      const r = await api.providers.setActive(p.id);
      flash(r.note);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(null);
    }
  };

  const remove = async (p: Provider) => {
    if (!window.confirm(`删除 provider「${p.name}」?其密钥将一并清除。`)) return;
    setBusy(`del:${p.id}`);
    try {
      await api.providers.remove(p.id);
      await reload();
      flash("已删除");
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-[15px] font-semibold">模型</h2>
          <p className="text-muted-foreground text-[12.5px]">
            OpenAI 兼容网关的增删改查;勾选「常用」= 对话输入框可选的模型子集;
            「设为当前」= 服务器默认(重启生效)。
          </p>
        </div>
        <Button onClick={() => setDraft({ ...emptyDraft })} size="sm">
          <PlusIcon /> 新增
        </Button>
      </div>

      {notice ? (
        <div className="rounded-lg border border-emerald-300 bg-emerald-50 px-3 py-2 text-[12.5px] text-emerald-700">
          {notice}
        </div>
      ) : null}
      {error ? (
        <div className="rounded-lg border border-red-300 bg-red-50 px-3 py-2 text-[12.5px] text-red-700">
          {error}
        </div>
      ) : null}

      <div className="flex flex-col gap-2">
        {providers.length === 0 ? (
          <div className="text-muted-foreground rounded-lg border border-dashed px-3 py-8 text-center text-[12.5px]">
            还没有 provider——点「新增」接入第一个网关。
          </div>
        ) : null}
        {providers.map((p) => {
          const probeResult = probes[p.id];
          return (
            <div
              key={p.id}
              className="bg-card flex flex-col gap-2 rounded-xl border p-3"
              data-provider={p.name}
            >
              <div className="flex items-center gap-2">
                <span className="min-w-0 flex-1 truncate text-[13.5px] font-medium">
                  {p.name}
                </span>
                {p.secretSet ? (
                  <Badge variant="secondary" className="font-mono text-[10px]">
                    密钥已存
                  </Badge>
                ) : (
                  <Badge variant="outline" className="font-mono text-[10px]">
                    无密钥
                  </Badge>
                )}
              </div>
              <div className="text-muted-foreground truncate font-mono text-[11.5px]">
                {p.baseUrl}
              </div>
              <div className="text-muted-foreground text-[12px]" data-common-summary>
                清单共 {p.models.length} 个
                {(p.modelsCommon?.length ?? 0) > 0
                  ? ` · 常用设置为:${p.modelsCommon!.join("、")}`
                  : p.defaultModel
                    ? ` · 默认 ${p.defaultModel}`
                    : ""}
              </div>

              <div className="flex flex-wrap items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy !== null}
                  onClick={() => void probe(p)}
                >
                  {busy === `probe:${p.id}` ? (
                    <Loader2Icon className="animate-spin" />
                  ) : (
                    <RefreshCwIcon />
                  )}
                  连通测试
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy !== null}
                  onClick={() => void setActive(p)}
                  title="写入服务器当前生效模型(重启后生效)"
                >
                  {busy === `active:${p.id}` ? (
                    <Loader2Icon className="animate-spin" />
                  ) : (
                    <PlugZapIcon />
                  )}
                  设为当前
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() =>
                    setDraft({
                      id: p.id,
                      name: p.name,
                      baseUrl: p.baseUrl,
                      apiKey: "",
                      models: p.models,
                      modelsCommon: p.modelsCommon ?? [],
                      defaultModel: p.defaultModel ?? "",
                    })
                  }
                >
                  <PencilIcon />
                  编辑
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-red-600 hover:text-red-700"
                  disabled={busy !== null}
                  onClick={() => void remove(p)}
                >
                  {busy === `del:${p.id}` ? (
                    <Loader2Icon className="animate-spin" />
                  ) : (
                    <Trash2Icon />
                  )}
                  删除
                </Button>
                <span className="flex-1" />
                {probeResult ? (
                  <span
                    className={cn(
                      "flex items-center gap-1 text-[12px]",
                      probeResult.ok ? "text-emerald-600" : "text-red-600",
                    )}
                    data-probe={probeResult.ok ? "ok" : "fail"}
                  >
                    {probeResult.ok ? (
                      <CheckCircle2Icon className="size-3.5" />
                    ) : (
                      <XCircleIcon className="size-3.5" />
                    )}
                    {probeResult.ok
                      ? `连通 ${probeResult.latencyMs}ms · 模型 ${probeResult.models?.length ?? 0} 个`
                      : `失败:${probeResult.error?.slice(0, 80) ?? probeResult.status}`}
                  </span>
                ) : null}
              </div>
            </div>
          );
        })}
      </div>

      <ProviderDialog
        draft={draft}
        onClose={() => setDraft(null)}
        onSaved={async (msg) => {
          setDraft(null);
          await reload();
          // W6:通知输入框模型下拉刷新(常用清单可能变了)
          window.dispatchEvent(new CustomEvent("bm-providers-changed"));
          flash(msg);
        }}
      />
    </div>
  );
}

function ProviderDialog({
  draft,
  onClose,
  onSaved,
}: {
  draft: Draft | null;
  onClose: () => void;
  onSaved: (msg: string) => void | Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [pulling, setPulling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<Draft | null>(draft);

  useEffect(() => {
    setForm(draft);
    setError(null);
  }, [draft]);

  if (!form) return null;
  const editing = Boolean(form.id);

  const pullModels = async () => {
    setPulling(true);
    setError(null);
    try {
      const r = await api.providers.probe(form.baseUrl, form.apiKey || undefined);
      if (!r.ok) {
        setError(`拉取失败:${r.error ?? `HTTP ${r.status}`}`);
        return;
      }
      const models = r.models ?? [];
      setForm((f) =>
        f
          ? {
              ...f,
              models,
              defaultModel: f.defaultModel || models[0] || "",
            }
          : f,
      );
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setPulling(false);
    }
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    const body = {
      name: form.name,
      baseUrl: form.baseUrl,
      apiKey: form.apiKey,
      models: form.models,
      modelsCommon: form.modelsCommon,
      defaultModel: form.defaultModel,
    };
    try {
      if (editing) {
        await api.providers.update(form.id!, body);
        void onSaved("已保存(密钥留空 = 保持不变)");
      } else {
        await api.providers.create(body);
        void onSaved("已新增 provider");
      }
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  const pickable: PickableModel[] = form.models.map((m) => ({
    id: m,
    name: m,
    family: "已拉取的模型清单",
    context: "",
    price: "",
    capabilities: [],
  }));

  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{editing ? "编辑提供商" : "新增提供商"}</DialogTitle>
          <DialogDescription>
            OpenAI 兼容网关;密钥只落服务器配置文件,回显恒打码。
            {editing ? "密钥留空 = 保持不变。" : ""}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-3">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="prov-name">名称</Label>
            <Input
              id="prov-name"
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="如 OpenCode Go"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="prov-url">Base URL</Label>
            <Input
              id="prov-url"
              value={form.baseUrl}
              onChange={(e) => setForm({ ...form, baseUrl: e.target.value })}
              placeholder="https://opencode.ai/zen/go/v1"
              className="font-mono"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="prov-key">API Key</Label>
            <Input
              id="prov-key"
              type="password"
              value={form.apiKey}
              onChange={(e) => setForm({ ...form, apiKey: e.target.value })}
              placeholder={editing ? "留空 = 保持不变" : "sk-…"}
              className="font-mono"
            />
          </div>

          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={pulling || !form.baseUrl}
              onClick={() => void pullModels()}
            >
              {pulling ? (
                <Loader2Icon className="animate-spin" />
              ) : (
                <RefreshCwIcon />
              )}
              连通并拉取模型清单
            </Button>
            <span className="text-muted-foreground text-[11.5px]">
              对 Base URL 发 GET /models(真实探针)
            </span>
          </div>

          {form.models.length > 0 ? (
            <>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="common-picker">
                  常用模型(可多选;对话输入框候选 = 各提供商常用并集)
                </Label>
                <div className="flex flex-wrap gap-1.5" data-slot="common-picker">
                  {form.models.map((m) => {
                    const on = form.modelsCommon.includes(m);
                    return (
                      <button
                        key={m}
                        id="common-picker"
                        type="button"
                        aria-pressed={on}
                        onClick={() =>
                          setForm({
                            ...form,
                            modelsCommon: on
                              ? form.modelsCommon.filter((x) => x !== m)
                              : [...form.modelsCommon, m],
                          })
                        }
                        className={cn(
                          "flex items-center gap-1 rounded-lg border px-2 py-1 font-mono text-[11.5px] transition-colors",
                          on
                            ? "border-emerald-300 bg-emerald-50 text-emerald-700"
                            : "text-muted-foreground hover:bg-muted",
                        )}
                      >
                        {on ? <CheckIcon className="size-3" /> : <PlusIcon className="size-3" />}
                        {m}
                      </button>
                    );
                  })}
                </div>
                <span className="text-muted-foreground text-[11.5px]">
                  已选 {form.modelsCommon.length} / {form.models.length}
                </span>
              </div>
              <div className="flex flex-col gap-1.5">
                <Label>默认模型(点选;「设为当前」用)</Label>
                <ModelPicker
                  models={pickable}
                  selectedId={form.defaultModel}
                  onSelect={(id) => setForm({ ...form, defaultModel: id })}
                  className="max-w-full"
                />
              </div>
            </>
          ) : null}

          {error ? (
            <div className="rounded-lg border border-red-300 bg-red-50 px-3 py-2 text-[12.5px] text-red-700">
              {error}
            </div>
          ) : null}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            取消
          </Button>
          <Button disabled={busy || !form.name || !form.baseUrl} onClick={() => void save()}>
            {busy ? <Loader2Icon className="animate-spin" /> : null}
            保存
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
