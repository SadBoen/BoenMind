// W2 设置中心 · MCP 管理页(结构沿插件管理同款:筛选 + 列表 + 操作)。
// 后端 = /admin/mcp(写 --mcp-config 配置文件,单条过 mcp-server.v0_1
// 冻结 schema;env 值必须 secret: 引用,明文不入配置 = INV-5 不倒退)。
// 增删改落盘后重启服务器生效(v0 诚实边界,页面明示)。
import { useCallback, useEffect, useState } from "react";
import {
  CogIcon,
  Loader2Icon,
  PencilIcon,
  PlusIcon,
  RefreshCwIcon,
  ScanSearchIcon,
  Trash2Icon,
} from "lucide-react";
import { api, type McpListResult, type McpServer } from "./api";
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

type McpManifestSchemaItem = {
  key: string;
  label: string;
  hint?: string;
  type: "string" | "secret" | "range" | "select";
  default?: string | number;
  min?: number;
  max?: number;
  unit?: string;
  options?: { value: string; label: string }[];
};

type ConfigTarget = {
  name: string;
  schema: McpManifestSchemaItem[];
  values: Record<string, unknown>;
};

// 插件目录扫描(两段式接入:扫描发现 → 批准接入落盘)
type McpCandidatesResult = {
  ok: boolean;
  dir: string;
  bundled_dir?: string | null;
  candidates: {
    file: string;
    name: string;
    title: string;
    description: string;
    registered: boolean;
    source?: string; // data=数据目录 mcp/ 手动放置;bundled=官方随包 plugins/
  }[];
  note: string;
};

type Draft = {
  name: string;
  transport: "stdio" | "sse" | "http";
  url: string;
  bearer_token: string;
  command: string;
  args: string; // 空格分隔
  env: string; // 每行 KEY=secret:ref
  tool_timeout_ms: string;
  _editing?: boolean; // true = 编辑既有条目(按原名 PUT)
};

const emptyDraft: Draft = {
  name: "",
  transport: "stdio",
  url: "",
  bearer_token: "",
  command: "",
  args: "",
  env: "",
  tool_timeout_ms: "",
};

function toDraft(s: McpServer): Draft {
  return {
    name: s.name,
    transport: s.transport === "http" || s.transport === "sse" ? s.transport : "stdio",
    url: s.url ?? "",
    bearer_token: s.bearer_token ?? "",
    command: s.command ?? "",
    args: (s.args ?? []).join(" "),
    env: Object.entries(s.env ?? {})
      .map(([k, v]) => `${k}=${v}`)
      .join("\n"),
    tool_timeout_ms: s.tool_timeout_ms ? String(s.tool_timeout_ms) : "",
    _editing: true,
  };
}

function fromDraft(d: Draft): Partial<McpServer> {
  const env: Record<string, string> = {};
  for (const line of d.env.split("\n")) {
    const i = line.indexOf("=");
    if (i <= 0) continue;
    const k = line.slice(0, i).trim();
    const v = line.slice(i + 1).trim();
    if (k && v) env[k] = v;
  }
  const base: Partial<McpServer> = {
    name: d.name.trim(),
    transport: d.transport,
    env,
    ...(d.tool_timeout_ms ? { tool_timeout_ms: Number(d.tool_timeout_ms) } : {}),
  };
  if (d.transport === "stdio") {
    base.command = d.command.trim();
    base.args = d.args.split(/\s+/).filter(Boolean);
  } else {
    base.url = d.url.trim();
    if (d.bearer_token.trim()) {
      base.bearer_token = d.bearer_token.trim();
    }
  }
  return base;
}

export function McpPage({
  editTarget,
  onConsumedEditTarget,
}: {
  editTarget?: string | null;
  onConsumedEditTarget?: () => void;
}) {
  const [filter, setFilter] = useState("");
  const [data, setData] = useState<McpListResult | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const [reloading, setReloading] = useState(false);
  // 联通状态(绿点):加载时拉一次,之后 30s 轮询 + 手动「测试」即时刷新
  const [statusMap, setStatusMap] = useState<Record<string, { ok: boolean; tools?: number; error?: string }>>({});
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [configTarget, setConfigTarget] = useState<ConfigTarget | null>(null);
  const [scanning, setScanning] = useState(false);
  const [approving, setApproving] = useState<string | null>(null);
  const [scanResult, setScanResult] = useState<McpCandidatesResult | null>(null);

  const scanPlugins = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      setScanResult(await api.mcp.candidates());
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setScanning(false);
    }
  }, []);

  const reload = useCallback(async () => {
    try {
      setData(await api.mcp.list());
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const refreshStatus = useCallback(async () => {
    try {
      const r = await api.mcp.status();
      const map: Record<string, { ok: boolean; tools?: number; error?: string }> = {};
      for (const st of r.status) map[st.name] = st;
      setStatusMap(map);
    } catch {
      /* 轮询失败静默(下一轮再试) */
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
    const t = setInterval(() => void refreshStatus(), 30_000);
    return () => clearInterval(t);
  }, [refreshStatus]);

  const testOne = useCallback(
    async (name: string) => {
      try {
        const r = await api.mcp.test(name);
        setStatusMap((cur) => ({ ...cur, [name]: r }));
      } catch (e) {
        setStatusMap((cur) => ({
          ...cur,
          [name]: { ok: false, error: String(e instanceof Error ? e.message : e) },
        }));
      }
    },
    [],
  );

  // 插件页「设置」跳转:直接打开对应条目编辑
  useEffect(() => {
    if (!editTarget || !data) return;
    const s = data.servers.find((x) => x.name === editTarget);
    if (s) setDraft(toDraft(s));
    else setError(`MCP server「${editTarget}」不在当前配置文件中(可能来自启动时装载的旧清单)`);
    onConsumedEditTarget?.();
  }, [editTarget, data, onConsumedEditTarget]);

  const flash = (msg: string) => {
    setNotice(msg);
    setError(null);
    setTimeout(() => setNotice(null), 6000);
  };

  const remove = async (name: string) => {
    if (!window.confirm(`移除 MCP server「${name}」?(落盘后点「重载 MCP」可免重启立即生效)`)) return;
    setBusy(true);
    try {
      await api.mcp.remove(name);
      await reload();
      flash("已从配置移除,点「重载 MCP」可免重启生效");
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  const shown = (data?.servers ?? []).filter((s) =>
    s.name.toLowerCase().includes(filter.trim().toLowerCase()),
  );

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-[15px] font-semibold">MCP 管理</h2>
          <p className="text-muted-foreground truncate text-[12.5px]">
            {data ? `配置文件 ${data.file}` : "…"};增删改落盘后点「重载 MCP」免重启生效。
          </p>
        </div>
        <span className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={scanning}
            onClick={() => void scanPlugins()}
            data-slot="mcp-scan"
          >
            {scanning ? (
              <Loader2Icon className="animate-spin" />
            ) : (
              <ScanSearchIcon />
            )}
            扫描插件
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={reloading}
            onClick={async () => {
              setReloading(true);
              try {
                const r = await api.mcp.reload();
                const parts: string[] = [];
                if (r.registered?.length) parts.push("新增: " + r.registered.join("、"));
                if (r.updated?.length) parts.push("更新: " + r.updated.join("、"));
                if (r.uninstalled?.length) parts.push("卸载: " + r.uninstalled.join("、"));
                setNotice(parts.length ? parts.join("; ") : (r.note ?? "无变更"));
                await reload();
                await refreshStatus();
              } catch (e) {
                setError(String(e instanceof Error ? e.message : e));
              } finally {
                setReloading(false);
              }
            }}
            data-slot="mcp-reload"
          >
            {reloading ? (
              <Loader2Icon className="animate-spin" />
            ) : (
              <RefreshCwIcon />
            )}
            重载 MCP
          </Button>
          <Button size="sm" onClick={() => setDraft({ ...emptyDraft })}>
            <PlusIcon /> 新增
          </Button>
        </span>
      </div>

      {data ? (
        <div className="text-muted-foreground text-[12px]">
          本次启动已装载:{data.loadedAtBoot.length ? data.loadedAtBoot.join("、") : "(无)"}
        </div>
      ) : null}

      <Input
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        placeholder="按名称筛选 MCP server…"
        className="max-w-xs"
      />

      {notice ? (
        <div className="notice-success">
          {notice}
        </div>
      ) : null}
      {error ? (
        <div className="notice-error">
          {error}
        </div>
      ) : null}

      <div className="flex flex-col gap-2">
        {shown.map((s) => {
          const entry = data?.entries?.find((e) => e.server.name === s.name);
          return (
          <div
            key={s.name}
            className="bg-card flex items-center gap-2 rounded-xl border p-3"
            data-mcp={s.name}
          >
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span
                  title={(statusMap[s.name]?.ok ?? false) ? "联通" : "未联通/未装载"}
                  data-slot="mcp-dot"
                  data-ok={statusMap[s.name]?.ok ? "1" : "0"}
                  className={
                    "size-2 shrink-0 rounded-full " +
                    (statusMap[s.name]?.ok ? "bg-emerald-500" : "bg-zinc-400")
                  }
                />
                <span className="truncate font-mono text-[13px]">{s.name}</span>
                <Badge variant="outline" className="text-[10px]">
                  stdio
                </Badge>
              </div>
              <div className="text-muted-foreground mt-0.5 truncate font-mono text-[11.5px]">
                {s.command} {(s.args ?? []).join(" ")}
              </div>
            </div>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setDraft(toDraft(s))}
            >
              <PencilIcon />
              编辑
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="text-red-600 hover:text-red-700"
              disabled={busy}
              onClick={() => void remove(s.name)}
            >
              <Trash2Icon />
              移除
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void testOne(s.name)}
              title="主动探活(tools/list)"
              data-slot="mcp-test"
            >
              测试
            </Button>
            {entry?.manifest?.config_schema?.length ? (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  api.mcp.getConfig(s.name).then((cfg) => {
                    setConfigTarget({
                      name: s.name,
                      schema: entry.manifest?.config_schema ?? [],
                      values: cfg.values,
                    });
                  });
                }}
              >
                <CogIcon />
                配置
              </Button>
            ) : null}
          </div>
          );
        })}
        {data && shown.length === 0 ? (
          <div className="text-muted-foreground rounded-lg border border-dashed px-3 py-8 text-center text-[12.5px]">
            {data.servers.length === 0
              ? "还没有 MCP server——点「新增」接入。"
              : `没有匹配「${filter}」的条目。`}
          </div>
        ) : null}
      </div>

      <ServerConfigDialog
        target={configTarget}
        onClose={() => setConfigTarget(null)}
      />

      {scanResult ? (
        <Dialog open onOpenChange={(v) => !v && setScanResult(null)}>
          <DialogContent className="sm:max-w-lg">
            <DialogHeader>
              <DialogTitle>插件目录扫描</DialogTitle>
              <DialogDescription>
                扫描目录:{scanResult.dir}
                {scanResult.bundled_dir
                  ? `;官方随包插件目录:${scanResult.bundled_dir}(随包插件免手动拷贝)`
                  : ""}
                。把插件可执行文件放进来,点「扫描插件」发现候选;批准后才接入。
              </DialogDescription>
            </DialogHeader>
            {scanResult.candidates.length ? (
              <div className="max-h-72 space-y-2 overflow-auto">
                {scanResult.candidates.map((c) => (
                  <div
                    key={c.name}
                    className="flex items-start justify-between gap-3 rounded-md border p-2"
                  >
                    <div className="min-w-0">
                      <div className="text-sm font-medium">
                        {c.title || c.name}{" "}
                        {c.source === "bundled" ? (
                          <span className="text-muted-foreground text-xs">(官方随包)</span>
                        ) : null}
                        {c.registered ? (
                          <span className="text-muted-foreground text-xs">(已登记)</span>
                        ) : null}
                      </div>
                      <div className="text-muted-foreground truncate text-xs">
                        {c.description || c.file}
                      </div>
                    </div>
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={c.registered || approving === c.name}
                      onClick={async () => {
                        setApproving(c.name);
                        try {
                          const r = await api.mcp.approve(c.name);
                          setScanResult(null);
                          await api.mcp.reload();
                          await reload();
                          await refreshStatus();
                          flash(r.note ?? "已接入");
                        } catch (e) {
                          setError(String(e instanceof Error ? e.message : e));
                        } finally {
                          setApproving(null);
                        }
                      }}
                      data-slot="mcp-approve"
                    >
                      {approving === c.name ? (
                        <Loader2Icon className="animate-spin" />
                      ) : null}
                      批准接入
                    </Button>
                  </div>
                ))}
              </div>
            ) : (
              <div className="text-muted-foreground text-sm">
                目录里没有可识别的插件(候选需支持 --self-describe 自报家门)。
              </div>
            )}
          </DialogContent>
        </Dialog>
      ) : null}

      <McpDialog
        draft={draft}
        onClose={() => setDraft(null)}
        onSaved={async (msg) => {
          setDraft(null);
          await reload();
          flash(msg);
        }}
      />
    </div>
  );
}

function McpDialog({
  draft,
  onClose,
  onSaved,
}: {
  draft: Draft | null;
  onClose: () => void;
  onSaved: (msg: string) => void | Promise<void>;
}) {
  const [form, setForm] = useState<Draft | null>(draft);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setForm(draft);
    setError(null);
  }, [draft]);

  if (!form) return null;

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const body = fromDraft(form);
      if (draft?._editing) {
        await api.mcp.update(draft.name, body);
        void onSaved("已更新,重启服务器后生效");
      } else {
        await api.mcp.create(body);
        void onSaved("已新增,重启服务器后生效");
      }
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{draft?._editing ? "编辑 MCP server" : "新增 MCP server"}</DialogTitle>
          <DialogDescription>
            名为小写(字母开头,仅小写字母/数字/下划线,至多 32 字符);env 值
            必须是 secret: 引用(明文不入配置);保存后重启服务器生效。
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="mcp-name">名称</Label>
            <Input
              id="mcp-name"
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="wiki / remote_service"
              className="font-mono"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="mcp-transport">传输协议</Label>
            <select
              id="mcp-transport"
              value={form.transport}
              onChange={(e) =>
                setForm({
                  ...form,
                  transport: e.target.value as "stdio" | "sse" | "http",
                })
              }
              className="border-input bg-background h-9 rounded-md border px-3 py-1 font-mono text-[13px] outline-none"
            >
              <option value="stdio">stdio (本地子进程)</option>
              <option value="sse">sse (远程流式/Server-Sent Events)</option>
              <option value="http">http (远程 Streamable HTTP)</option>
            </select>
          </div>

          {form.transport === "stdio" ? (
            <>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="mcp-cmd">启动命令</Label>
                <Input
                  id="mcp-cmd"
                  value={form.command}
                  onChange={(e) => setForm({ ...form, command: e.target.value })}
                  placeholder="uvx / node / npx …"
                  className="font-mono"
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="mcp-args">参数(空格分隔)</Label>
                <Input
                  id="mcp-args"
                  value={form.args}
                  onChange={(e) => setForm({ ...form, args: e.target.value })}
                  placeholder="mcp-wiki --port 3000"
                  className="font-mono"
                />
              </div>
            </>
          ) : (
            <>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="mcp-url">远程服务 URL</Label>
                <Input
                  id="mcp-url"
                  value={form.url}
                  onChange={(e) => setForm({ ...form, url: e.target.value })}
                  placeholder="https://api.example.com/mcp"
                  className="font-mono"
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="mcp-bearer">访问令牌 (可选, 格式 secret:ref)</Label>
                <Input
                  id="mcp-bearer"
                  value={form.bearer_token}
                  onChange={(e) => setForm({ ...form, bearer_token: e.target.value })}
                  placeholder="secret:remote-token"
                  className="font-mono"
                />
              </div>
            </>
          )}

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="mcp-env">环境变量(每行 KEY=secret:引用)</Label>
            <textarea
              id="mcp-env"
              value={form.env}
              onChange={(e) => setForm({ ...form, env: e.target.value })}
              placeholder={"WIKI_TOKEN=secret:wiki-token"}
              className="border-input bg-background focus-visible:ring-ring h-20 rounded-md border px-3 py-2 font-mono text-[12.5px] outline-none focus-visible:ring-2"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="mcp-timeout">工具超时(ms,可选)</Label>
            <Input
              id="mcp-timeout"
              value={form.tool_timeout_ms}
              onChange={(e) => setForm({ ...form, tool_timeout_ms: e.target.value })}
              placeholder="30000"
              className="font-mono"
            />
          </div>
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
          <Button disabled={busy || !form.name || !form.command} onClick={() => void save()}>
            {busy ? <Loader2Icon className="animate-spin" /> : null}
            保存
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}


/// 每 server 自声明配置表单(manifest.config_schema 动态渲染;
/// 保存写 config/mcp-<name>.json,server 侧按 override 链消费)。
function ServerConfigDialog({
  target,
  onClose,
}: {
  target: ConfigTarget | null;
  onClose: () => void;
}) {
  const [values, setValues] = useState<Record<string, unknown> | null>(null);
  const [picked, setPicked] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    setValues(target ? { ...target.values } : null);
    setPicked(target?.schema?.[0]?.key ?? "");
    setError(null);
    setNotice(null);
  }, [target]);

  if (!target || !target.schema) return null;

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.mcp.saveConfig(target.name, values ?? {});
      setNotice(r.note);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{"配置 · " + target.name}</DialogTitle>
          <DialogDescription>
            由插件 manifest 自声明;改 Key 立即生效,其余项重载/重启生效。
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          {notice ? (
            <div className="rounded-lg border border-emerald-300 bg-emerald-50 px-3 py-2 text-[12.5px] text-emerald-700">
              {notice}
            </div>
          ) : null}
          {/* 用户裁定:配置项用下拉选择器(选一项,下方输入跟着变) */}
          <div className="flex flex-col gap-1.5">
            <Label>配置项</Label>
            <select
              className="border-input bg-background h-8 rounded-md border px-2 text-[12.5px] outline-none"
              value={picked}
              onChange={(e) => setPicked(e.target.value)}
              data-slot="config-pick"
            >
              {target.schema.map((f) => (
                <option key={f.key} value={f.key}>
                  {f.label}
                </option>
              ))}
            </select>
          </div>
          {(() => {
            const f =
              target.schema.find((x) => x.key === picked) ?? target.schema[0];
            if (!f) return null;
            const v = values ? values[f.key] : undefined;
            return (
              <div className="flex flex-col gap-1.5">
                <Label>{f.label}</Label>
                {f.type === "range" ? (
                  <Input
                    type="number"
                    min={f.min}
                    max={f.max}
                    value={Number(v ?? f.default ?? 0)}
                    onChange={(e) =>
                      setValues({ ...values, [f.key]: Number(e.target.value) })
                    }
                    className="font-mono"
                  />
                ) : (
                  <Input
                    type={f.type === "secret" ? "password" : "text"}
                    value={String(v ?? f.default ?? "")}
                    onChange={(e) =>
                      setValues({ ...values, [f.key]: e.target.value })
                    }
                    className="font-mono"
                  />
                )}
                {f.hint ? (
                  <div className="text-muted-foreground text-[11.5px]">{f.hint}</div>
                ) : null}
              </div>
            );
          })()}
          {error ? (
            <div className="rounded-lg border border-red-300 bg-red-50 px-3 py-2 text-[12.5px] text-red-700">
              {error}
            </div>
          ) : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            关闭
          </Button>
          <Button disabled={busy} onClick={() => void save()}>
            {busy ? <Loader2Icon className="animate-spin" /> : null}
            保存
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
