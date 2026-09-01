# -*- coding: utf-8 -*-
"""McpPage 补丁:重载按钮 + 每 server 自声明配置表单。"""
import io, sys

p = 'src/w2/McpPage.tsx'
s = io.open(p, encoding='utf-8').read()

# 1) lucide imports
s = s.replace(
    'import { Loader2Icon, PencilIcon, PlusIcon, Trash2Icon } from "lucide-react";',
    'import {\n  ConfigIcon,\n  Loader2Icon,\n  PencilIcon,\n  PlusIcon,\n  RefreshCwIcon,\n  Trash2Icon,\n} from "lucide-react";',
    1)

# 2) state 扩展
old_state = '''  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);'''
new_state = '''  const [busy, setBusy] = useState(false);
  const [reloading, setReloading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [configTarget, setConfigTarget] = useState<ConfigTarget | null>(null);'''
assert old_state in s
s = s.replace(old_state, new_state, 1)

# 3) 顶部按钮组:重载 + 新增
old_btn = '''        <Button size="sm" onClick={() => setDraft({ ...emptyDraft })}>
          <PlusIcon /> 新增
        </Button>
      </div>'''
new_btn = '''        <span className="flex items-center gap-2">
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
      </div>'''
assert old_btn in s
s = s.replace(old_btn, new_btn, 1)

# 4) 条目行:map 体内 + 配置按钮
old_row_open = '''        {shown.map((s) => (
          <div
            key={s.name}
            className="bg-card flex items-center gap-2 rounded-xl border p-3"
            data-mcp={s.name}
          >'''
new_row_open = '''        {shown.map((s) => {
          const entry = data?.entries?.find((e) => e.server.name === s.name);
          return (
          <div
            key={s.name}
            className="bg-card flex items-center gap-2 rounded-xl border p-3"
            data-mcp={s.name}
          >'''
assert old_row_open in s
s = s.replace(old_row_open, new_row_open, 1)

old_row_close = '''              移除
            </Button>
          </div>
        ))}'''
new_row_close = '''              移除
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
                <ConfigIcon />
                配置
              </Button>
            ) : null}
          </div>
          );
        })}'''
assert old_row_close in s
s = s.replace(old_row_close, new_row_close, 1)

# 5) 挂配置 Dialog
old_dialog = '''      <McpDialog
        draft={draft}
        onClose={() => setDraft(null)}'''
new_dialog = '''      <ServerConfigDialog
        target={configTarget}
        onClose={() => setConfigTarget(null)}
      />

      <McpDialog
        draft={draft}
        onClose={() => setDraft(null)}'''
assert old_dialog in s
s = s.replace(old_dialog, new_dialog, 1)

# 6) 尾部追加 ServerConfigDialog
tail = '''

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
'''
s += tail

# ConfigTarget 类型(api.ts 的 manifest schema 局部类型在此声明引用)
s = s.replace('''type Draft = {
  name: string;''', '''type McpManifestSchemaItem = {
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
  name: string;''')

io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('patched ok')
