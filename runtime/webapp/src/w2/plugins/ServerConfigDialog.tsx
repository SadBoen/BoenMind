//! MCP server 配置弹窗(自 PluginsPage.tsx 机械移入)。
import { useEffect, useMemo, useState } from "react";
import {
  Loader2Icon,
  PlusIcon,
  PlayIcon,
  XIcon,
} from "lucide-react";
import {
  api,
  type ProviderManifestItem,
} from "../api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { Switch } from "@/components/ui/switch";
import { FormField } from "./FormField";
import { type ProviderEntry, type ConfigTarget } from "./types";

export function ServerConfigDialog({
  target,
  onClose,
}: {
  target: ConfigTarget | null;
  onClose: () => void;
}) {
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [providers, setProviders] = useState<ProviderEntry[]>([]);
  const [selectedId, setSelectedId] = useState<string>("");
  const [usage, setUsage] = useState<Record<string, number>>({});
  const [usageMonth, setUsageMonth] = useState<string>("");
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testQuery, setTestQuery] = useState("");
  const [testLimit, setTestLimit] = useState(5);
  const [testResult, setTestResult] = useState<
    { ok: boolean; title: string; body: string } | null
  >(null);
  const [err, setErr] = useState<string | null>(null);

  // 从 schema 里取 providers 描述(若无则非 providers 型,退回普通表单)
  const providerSchema = useMemo(
    () => target?.schema.find((s) => s.type === "providers") ?? null,
    [target],
  );
  const isProviders = !!providerSchema;

  // 初始化:合并已存 providers 值 + manifest 内置模板
  useEffect(() => {
    if (!target) return;
    setValues(target.values || {});
    setErr(null);
    setTestResult(null);

    if (!providerSchema) {
      setProviders([]);
      setSelectedId("");
      return;
    }
    const templates: ProviderManifestItem[] = providerSchema.items ?? [];
    const stored: unknown[] = Array.isArray(target.values?.providers)
      ? (target.values.providers as unknown[])
      : [];
    // 已存条目按 id 索引(自定义优先;内置若被改动则采用已存版)
    const byId = new Map<string, ProviderEntry>();
    for (const t of templates) {
      byId.set(t.id, { ...t, enabled: true, present: false });
    }
    for (const s of stored) {
      if (s && typeof s === "object") {
        const obj = s as Record<string, unknown>;
        const id = String(obj.id ?? "");
        if (id) {
          const base = byId.get(id) ?? ({} as ProviderEntry);
          byId.set(id, { ...base, ...obj, id, present: true } as ProviderEntry);
        }
      }
    }
    // 已存里出现、但模板没有的自定义供应商(全新),补进去
    for (const s of stored) {
      if (s && typeof s === "object") {
        const obj = s as Record<string, unknown>;
        const id = String(obj.id ?? "");
        if (id && !byId.has(id)) {
          byId.set(id, { ...(obj as ProviderEntry), id, builtin: false });
        }
      }
    }
    const list = Array.from(byId.values());
    // 已删墓碑排到最后,列表主序保持可用家在前
    list.sort((a, b) => Number(!!a.deleted) - Number(!!b.deleted));
    setProviders(list);
    finalizeSelection(list, target.values, usage);
  }, [target, providerSchema]);

  // 默认选中第一个可用(未删)家;停用家可选,已删家不默认选中
  function finalizeSelection(
    list: ProviderEntry[],
    vals: Record<string, unknown>,
    _u: Record<string, number>,
  ) {
    if (list.length) {
      const live = list.filter((p) => !p.deleted);
      const storedIds = new Set(
        (Array.isArray(vals.providers) ? vals.providers : []).map(
          (x: unknown) => (x as Record<string, unknown>).id as string,
        ),
      );
      const firstStored = live.find((p) => storedIds.has(p.id)) ?? live[0];
      if (firstStored) setSelectedId(firstStored.id);
    }
  }

  // 拉取用量(进度条)
  useEffect(() => {
    if (!target || !isProviders) return;
    let alive = true;
    api.mcp
      .getUsage(target.name)
      .then((r) => {
        if (!alive) return;
        setUsage(r.usage?.providers ?? {});
        setUsageMonth(r.usage?.month ?? "");
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [target, isProviders]);

  if (!target) return null;

  const selected = providers.find((p) => p.id === selectedId) ?? null;

  const setField = (id: string, field: keyof ProviderManifestItem, val: string | boolean | number) =>
    setProviders((prev) =>
      prev.map((p) => (p.id === id ? { ...p, [field]: val } : p)),
    );

  // 新增家:生成随机 id,强制走通用引擎(std)
  const handleAdd = () => {
    const id = `custom_${Date.now().toString(36)}`;
    const fresh: ProviderEntry = {
      id,
      name: "新供应商",
      builtin: false,
      enabled: true,
      endpoint: "",
      method: "GET",
      auth: "header",
      auth_name: "X-API-KEY",
      key: "",
      query_param: "q",
      limit_param: "",
      results_path: "/results",
      title_field: "title",
      url_field: "url",
      desc_field: "description",
      parse: "std",
      quota: 0,
      present: true,
    };
    setProviders((prev) => [...prev, fresh]);
    setSelectedId(id);
  };

  const handleTest = async () => {
    if (!selected || !testQuery) return;
    setTesting(true);
    setTestResult(null);
    setErr(null);
    try {
      const r = await api.mcp.testSearch(
        target.name,
        selected.id,
        testQuery,
        testLimit || 5,
      );
      if (!r.ok) {
        setTestResult({ ok: false, title: "测试失败", body: r.error ?? "未知错误" });
        return;
      }
      const res = r.result;
      if (!res?.success) {
        setTestResult({
          ok: false,
          title: `${selected.id} 测试失败`,
          body: res?.error ?? "无返回",
        });
      } else {
        const lines = (res.results ?? [])
          .slice(0, 10)
          .map((it, i) => `${i + 1}. ${it.title ?? ""}\n   ${it.url ?? ""}\n   ${it.description ?? ""}`)
          .join("\n");
        setTestResult({
          ok: true,
          title: `${selected.name} · ${res.count ?? 0} 条 · ${res.timing_ms ?? 0}ms`,
          body: lines || "(无结果)",
        });
      }
      // 测试成功会记一次用量,顺手刷新
      if (res?.success) {
        api.mcp.getUsage(target.name).then((qr) => {
          if (qr.ok && qr.usage?.providers) setUsage(qr.usage.providers);
        });
      }
    } catch (e) {
      setTestResult({
        ok: false,
        title: "测试请求失败",
        body: String(e instanceof Error ? e.message : e),
      });
    } finally {
      setTesting(false);
    }
  };

  const handleSave = async () => {
    setSaving(true);
    setErr(null);
    try {
      if (isProviders) {
        // 保存 providers 列表(过滤掉空 id;present 是 UI 专用字段不落盘,
        // enabled/deleted 墓碑保留——插件侧据此跳过停用家/抑制内置回填)
        const persist = providers
          .filter((p) => p.id)
          .map(({ present: _present, ...rest }) => rest);
        setValues((prev) => ({ ...prev, providers: persist }));
        await api.mcp.saveConfig(target.name, {
          ...values,
          providers: persist,
        });
      } else {
        await api.mcp.saveConfig(target.name, values);
      }
      onClose();
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setSaving(false);
    }
  };

  // 若 providers 型 → 左右分栏新布局
  if (isProviders) {
    return (
      <Dialog open onOpenChange={(v) => !v && onClose()}>
        <DialogContent className="flex h-[min(82vh,620px)] flex-col overflow-hidden sm:max-w-[798px]">
          <DialogHeader className="shrink-0">
            <div className="flex items-start justify-between gap-3 pr-4">
              <div className="min-w-0">
                <DialogTitle>配置 · {target.name}</DialogTitle>
                <DialogDescription>
                  左侧点选供应商编辑,「新增」接入全新搜索服务(通用引擎)。
                </DialogDescription>
              </div>
              <Button
                variant="outline"
                size="sm"
                className="mt-0.5 shrink-0 gap-1"
                onClick={handleAdd}
                title="新增供应商(通用引擎)"
                data-slot="provider-add"
              >
                <PlusIcon className="size-3.5" /> 新增
              </Button>
            </div>
          </DialogHeader>

          {/* 统一容器:固定高度 flex 列,左右在内部滚动,对话框高度恒定 */}
          <div className="min-h-0 flex-1 overflow-hidden rounded-xl border">
            <div className="grid h-full grid-cols-[minmax(0,210px)_minmax(0,1fr)]">
              {/* ===== 左:用量进度条(内部滚动,滚动条隐藏) ===== */}
              <div className="bo-scroll-hidden flex h-full min-h-0 flex-col overflow-y-auto border-r p-2.5">
                <div className="flex shrink-0 items-center justify-between">
                  <Label className="text-xs font-semibold">用量(本月)</Label>
                  {usageMonth ? (
                    <span className="text-muted-foreground text-[10.5px] font-mono">
                      {usageMonth}
                    </span>
                  ) : null}
                </div>
                {/* 列表在左栏内部滚动 */}
                <div className="mt-1.5 space-y-1.5">
                {providers.map((p) => {
                  const used = usage[p.id] ?? 0;
                  const quota = p.quota ?? 0;
                  const ratio = quota > 0 ? Math.min(1, used / quota) : 0;
                  const pct = Math.round(ratio * 100);
                  const color =
                    ratio >= 1
                      ? "bg-rose-500"
                      : ratio >= 0.8
                        ? "bg-amber-500"
                        : "bg-emerald-500";
                  const off = p.enabled === false || !!p.deleted;
                  return (
                    <button
                      key={p.id}
                      type="button"
                      onClick={() => setSelectedId(p.id)}
                      className={`w-full text-left rounded-md border px-2.5 py-1.5 transition-colors ${
                        selected?.id === p.id
                          ? "border-primary/60 bg-primary/5"
                          : "border-border bg-card/40 hover:bg-muted/40"
                      } ${off ? "opacity-60" : ""}`}
                    >
                      <div className="flex items-baseline justify-between gap-2">
                        <span className={`truncate text-[12px] font-medium ${off ? "text-muted-foreground line-through decoration-border" : ""}`}>
                          {p.name || p.id}
                        </span>
                        <span className="text-muted-foreground shrink-0 text-[10.5px] font-mono">
                          {quota > 0 ? `${used}/${quota}` : `${used} 次`}
                          {quota > 0 ? ` · ${pct}%` : ""}
                        </span>
                      </div>
                      <div className="mt-1 flex items-center gap-1.5">
                        {p.deleted ? (
                          <span className="shrink-0 rounded border border-border px-1 font-mono text-[9.5px] text-muted-foreground">
                            已删除
                          </span>
                        ) : p.enabled === false ? (
                          <span className="shrink-0 rounded border border-[var(--state-warn-border)] bg-[var(--state-warn-bg)] px-1 font-mono text-[9.5px] text-[var(--state-warn-fg)]">
                            已停用
                          </span>
                        ) : null}
                        <div className="h-1 min-w-0 flex-1 overflow-hidden rounded-full bg-muted">
                          <div
                            className={`h-full rounded-full transition-all ${color}`}
                            style={{ width: `${quota > 0 ? pct : 0}%` }}
                          />
                        </div>
                      </div>
                    </button>
                  );
                })}
              </div>
            </div>

            {/* ===== 右:供应商配置(内部滚动,滚动条隐藏) ===== */}
            <div className="bo-scroll-hidden flex min-h-0 min-w-0 flex-col gap-2.5 overflow-y-auto p-2.5">
              {/* 通用设置(全局,与供应商无关) */}
              <div className="rounded-lg border p-2.5">
                <div className="flex items-center justify-between">
                  <Label className="text-[11px] font-semibold">通用设置</Label>
                  <span className="text-muted-foreground text-[10.5px]">指最终返回,非每家条数</span>
                </div>
                <div className="mt-1.5 flex flex-wrap items-center gap-x-4 gap-y-1">
                  {target.schema
                    .filter((s) => s.type !== "providers")
                    .map((item) => (
                      <div key={item.key} className="space-y-0.5">
                        <Label className="text-[11px] font-medium">
                          {item.key === "default_limit"
                            ? "最终返回条数"
                            : item.label || item.key}
                        </Label>
                        <div className="h-7">
                          <Input
                            type={item.type === "range" ? "number" : "text"}
                            value={String(values[item.key] ?? item.default ?? "")}
                            onChange={(e) =>
                              setValues((prev) => ({ ...prev, [item.key]: e.target.value }))
                            }
                            className="h-7 w-24 font-mono text-[12px]"
                          />
                        </div>
                      </div>
                    ))}
                </div>
              </div>
              {/* 选中家:已删 → 恢复卡;未删 → 生效开关 + 删除 + 编辑表单 */}
              {selected && selected.deleted ? (
                <div className="rounded-lg border border-dashed p-4">
                  <div className="flex items-center justify-between gap-3">
                    <div className="min-w-0">
                      <div className="text-[12.5px] font-medium">
                        {selected.builtin ? "内置供应商" : "供应商"}「
                        {selected.name || selected.id}」已删除
                      </div>
                      <div className="text-muted-foreground mt-0.5 text-[11px]">
                        删除后不参与搜索轮转;历史用量保留,恢复后按原配置继续生效。
                      </div>
                    </div>
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 shrink-0 text-[12px]"
                      onClick={() =>
                        setProviders((prev) =>
                          prev.map((p) =>
                            p.id === selected.id
                              ? { ...p, deleted: false, enabled: true }
                              : p,
                          ),
                        )
                      }
                      data-slot="provider-restore"
                    >
                      恢复
                    </Button>
                  </div>
                </div>
              ) : selected ? (
                <div className="rounded-lg border p-3">
                  <div className="mb-2.5 flex items-center justify-between gap-2">
                    <Label className="text-xs font-semibold">
                      {selected.builtin ? "编辑内置" : "编辑自定义"}:{" "}
                      {selected.name || selected.id}
                    </Label>
                    <div className="flex shrink-0 items-center gap-2.5">
                      <span className="text-muted-foreground flex items-center gap-1.5 text-[11px]">
                        <Switch
                          checked={selected.enabled !== false}
                          onCheckedChange={(v) =>
                            setField(selected.id, "enabled", v)
                          }
                          data-slot="provider-enabled"
                        />
                        {selected.enabled === false ? "已停用" : "生效中"}
                      </span>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="text-destructive h-6 px-2 text-[11px]"
                        title={
                          selected.builtin
                            ? "删除该内置供应商(存墓碑,可恢复)"
                            : "移除该自定义供应商"
                        }
                        data-slot="provider-remove"
                        onClick={() => {
                          if (selected.builtin) {
                            // 内置:存墓碑条目抑制插件「缺失内置回填」,可恢复
                            setProviders((prev) =>
                              prev.map((p) =>
                                p.id === selected.id
                                  ? { ...p, deleted: true }
                                  : p,
                              ),
                            );
                          } else {
                            setProviders((prev) =>
                              prev.filter((p) => p.id !== selected.id),
                            );
                          }
                          const next = providers.find(
                            (p) => p.id !== selected.id && !p.deleted,
                          );
                          setSelectedId(next?.id ?? "");
                        }}
                      >
                        {selected.builtin ? "删除" : "移除"}
                      </Button>
                    </div>
                  </div>
                  <div className="grid grid-cols-2 gap-3 text-[12px]">
                    <FormField
                      label="显示名"
                      value={selected.name}
                      onChange={(v) => setField(selected.id, "name", v)}
                    />
                    <FormField
                      label="接口地址"
                      value={selected.endpoint}
                      mono
                      placeholder="https://api.example.com/search"
                      onChange={(v) => setField(selected.id, "endpoint", v)}
                    />
                    <div className="space-y-1">
                      <Label className="text-[11px]">请求方式</Label>
                      <Select
                        value={selected.method}
                        onValueChange={(v) => v && setField(selected.id, "method", v)}
                      >
                        <SelectTrigger size="sm" className="h-7 w-full text-[12px]">
                          <SelectValue placeholder="方式" />
                        </SelectTrigger>
                        <SelectContent className="rounded-lg">
                          <SelectItem value="GET">GET</SelectItem>
                          <SelectItem value="POST">POST</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <FormField
                      label="API Key(逗号多把)"
                      value={selected.key ?? ""}
                      password
                      mono
                      onChange={(v) => setField(selected.id, "key", v)}
                    />
                    <div className="space-y-1">
                      <Label className="text-[11px]">Key 传法</Label>
                      <Select
                        value={selected.auth}
                        onValueChange={(v) => v && setField(selected.id, "auth", v)}
                      >
                        <SelectTrigger size="sm" className="h-7 w-full text-[12px]">
                          <SelectValue placeholder="传法" />
                        </SelectTrigger>
                        <SelectContent className="rounded-lg">
                          <SelectItem value="header">请求头</SelectItem>
                          <SelectItem value="bearer">Bearer Token</SelectItem>
                          <SelectItem value="query">参数 (query/body)</SelectItem>
                          <SelectItem value="none">无</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    {selected.auth === "header" || selected.auth === "query" ? (
                      <FormField
                        label={selected.auth === "header" ? "请求头名" : "Key 参数名"}
                        value={selected.auth_name}
                        mono
                        placeholder={selected.auth === "header" ? "X-API-KEY" : "api_key"}
                        onChange={(v) => setField(selected.id, "auth_name", v)}
                      />
                    ) : null}
                    <FormField
                      label="query 参数名"
                      value={selected.query_param}
                      mono
                      placeholder="q"
                      onChange={(v) => setField(selected.id, "query_param", v)}
                    />
                    <FormField
                      label="limit 参数名(可空)"
                      value={selected.limit_param}
                      mono
                      placeholder="num / max_results"
                      onChange={(v) => setField(selected.id, "limit_param", v)}
                    />
                    <FormField
                      label="结果数组位置(JSON 路径)"
                      value={selected.results_path}
                      mono
                      placeholder="/organic"
                      onChange={(v) => setField(selected.id, "results_path", v)}
                    />
                    <div className="space-y-1">
                      <Label className="text-[11px]">月度配额(0=不限)</Label>
                      <Input
                        type="number"
                        min={0}
                        value={String(selected.quota ?? 0)}
                        onChange={(e) =>
                          setField(selected.id, "quota", Number(e.target.value) || 0)
                        }
                        className="h-7 font-mono text-[12px]"
                      />
                    </div>
                    <FormField
                      label="标题字段"
                      value={selected.title_field}
                      mono
                      placeholder="title"
                      onChange={(v) => setField(selected.id, "title_field", v)}
                    />
                    <FormField
                      label="链接字段"
                      value={selected.url_field}
                      mono
                      placeholder="url"
                      onChange={(v) => setField(selected.id, "url_field", v)}
                    />
                    <FormField
                      label="摘要字段"
                      value={selected.desc_field}
                      mono
                      placeholder="snippet"
                      onChange={(v) => setField(selected.id, "desc_field", v)}
                    />
                  </div>

                  {/* 测试按钮 */}
                  <div className="mt-3 flex items-center gap-2 border-t pt-3">
                    <Input
                      value={testQuery}
                      onChange={(e) => setTestQuery(e.target.value)}
                      placeholder="输入测试关键词…"
                      className="h-8 flex-1 text-[12px]"
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void handleTest();
                      }}
                    />
                    <Input
                      type="number"
                      min={1}
                      max={20}
                      value={String(testLimit || 5)}
                      onChange={(e) => setTestLimit(Number(e.target.value) || 5)}
                      className="h-8 w-16 text-[12px]"
                      title="返回条数"
                    />
                    <Button
                      variant="secondary"
                      size="sm"
                      className="h-8 shrink-0 gap-1.5"
                      onClick={() => void handleTest()}
                      disabled={testing || !testQuery}
                    >
                      {testing ? (
                        <Loader2Icon className="animate-spin size-3.5" />
                      ) : (
                        <PlayIcon className="size-3.5" />
                      )}
                      测试真搜
                    </Button>
                  </div>

                  {testResult ? (
                    <div
                      className={cn(
                        "mt-2.5 rounded-md border p-2.5 text-[12px]",
                        testResult.ok
                          ? "border-[var(--state-success-border)] bg-[var(--state-success-bg)]"
                          : "border-[var(--state-error-border)] bg-[var(--state-error-bg)]",
                      )}
                    >
                      <div className="mb-1 flex items-center justify-between font-medium">
                        <span
                          className={
                            testResult.ok
                              ? "text-[var(--state-success-fg)]"
                              : "text-[var(--state-error-fg)]"
                          }
                        >
                          {testResult.ok ? "✓ " : "✕ "}
                          {testResult.title}
                        </span>
                        <button
                          className="text-muted-foreground transition-colors hover:text-foreground"
                          onClick={() => setTestResult(null)}
                          title="收起结果"
                        >
                          <XIcon className="size-3.5" />
                        </button>
                      </div>
                      <pre className="max-h-48 overflow-auto whitespace-pre-wrap font-mono text-[11px] leading-relaxed">
                        {testResult.body}
                      </pre>
                    </div>
                  ) : null}
                </div>
              ) : (
                <div className="text-muted-foreground rounded-lg border border-dashed p-6 text-center text-[12px]">
                  点「新增」接入全新搜索供应商
                </div>
              )}
            </div>
            </div>
          </div>

          {err ? <div className="text-destructive text-xs">{err}</div> : null}
          <DialogFooter className="shrink-0">
            <Button variant="outline" onClick={onClose} disabled={saving}>
              取消
            </Button>
            <Button onClick={() => void handleSave()} disabled={saving}>
              {saving ? <Loader2Icon className="animate-spin" /> : null} 保存配置
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }

  // ===== 普通(非 providers)插件:保持原瀑布式表单 =====
  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>配置 · {target.name}</DialogTitle>
          <DialogDescription>该插件声明了可调节的运行配置。</DialogDescription>
        </DialogHeader>
        <div className="space-y-3 py-2">
          {target.schema.map((item) => (
            <div key={item.key} className="space-y-1">
              <Label className="text-xs font-medium">
                {item.label || item.key}
                {item.hint ? (
                  <span className="text-muted-foreground ml-1 font-normal">
                    ({item.hint})
                  </span>
                ) : null}
              </Label>
              <Input
                value={String(values[item.key] ?? item.default ?? "")}
                onChange={(e) =>
                  setValues((prev) => ({ ...prev, [item.key]: e.target.value }))
                }
                className="h-8 font-mono text-xs"
              />
            </div>
          ))}
          {err ? <div className="text-destructive text-xs">{err}</div> : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={saving}>
            取消
          </Button>
          <Button onClick={() => void handleSave()} disabled={saving}>
            {saving ? <Loader2Icon className="animate-spin" /> : null} 保存配置
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// 一个小型表单字段(标签 + 单行输入),供 providers 编辑复用
