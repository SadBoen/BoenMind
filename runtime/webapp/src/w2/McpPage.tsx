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

type Draft = {
  name: string;
  command: string;
  args: string; // 空格分隔
  env: string; // 每行 KEY=secret:ref
  tool_timeout_ms: string;
  _editing?: boolean; // true = 编辑既有条目(按原名 PUT)
};

const emptyDraft: Draft = {
  name: "",
  command: "",
  args: "",
  env: "",
  tool_timeout_ms: "",
};

function toDraft(s: McpServer): Draft {
  return {
    name: s.name,
    command: s.command,
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
  return {
    name: d.name.trim(),
    command: d.command.trim(),
    args: d.args.split(/\s+/).filter(Boolean),
    env,
    ...(d.tool_timeout_ms ? { tool_timeout_ms: Number(d.tool_timeout_ms) } : {}),
  };
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
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [configTarget, setConfigTarget] = useState<ConfigTarget | null>(null);

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
    if (!window.confirm(`移除 MCP server「${name}」?(重启生效)`)) return;
    setBusy(true);
    try {
      await api.mcp.remove(name);
      await reload();
      flash("已移除,重启服务器后生效");
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
            {data ? `配置文件 ${data.file}` : "…"};增删改落盘后重启生效。
          </p>
        </div>
        <span className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={reloading}
            onClick={async () => {
              setReloading(true);
              try {
                const r = await api.mcp.reload();
                setNotice(
                  r.registered.length
                    ? "已装载新增: " + r.registered.join("、")
                    : (r.note ?? "无新增"),
                );
                await reload();
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
              placeholder="wiki"
              className="font-mono"
            />
          </div>
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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    setValues(target ? { ...target.values } : null);
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
          {target.schema.map((f) => {
            const v = values ? values[f.key] : undefined;
            return (
              <div key={f.key} className="flex flex-col gap-1.5">
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
          })}
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
