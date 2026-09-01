# -*- coding: utf-8 -*-
"""设置页新增「角色」分区(prompt 编辑;工具策略只读展示)。"""
import io

p = 'src/w2/SettingsPage.tsx'
s = io.open(p, encoding='utf-8', newline='').read()

# 1) 导航加「角色」
s = s.replace('''          <NavItem
            active={section === "appearance"}
            onClick={() => setSection("appearance")}
            icon={<SparklesIcon className="size-4" />}
            label="外观"
            hint="W3"
          />''', '''          <NavItem
            active={section === "appearance"}
            onClick={() => setSection("appearance")}
            icon={<SparklesIcon className="size-4" />}
            label="外观"
            hint="W3"
          />
          <NavItem
            active={section === "roles"}
            onClick={() => setSection("roles")}
            icon={<BoxIcon className="size-4" />}
            label="角色"
            hint="W4"
          />''')

# 2) Section 类型 + 内容分支
s = s.replace('''type Section = "providers" | "plugins" | "mcp" | "appearance";''',
'''type Section = "providers" | "plugins" | "mcp" | "appearance" | "roles";''')
s = s.replace('''import { McpPage } from "./McpPage";''',
'''import { McpPage } from "./McpPage";
import { RolesPage } from "./RolesPage";''')
s = s.replace('''            {section === "appearance" ? <AppearancePage /> : null}''',
'''            {section === "appearance" ? <AppearancePage /> : null}
            {section === "roles" ? <RolesPage /> : null}''')

io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('settings nav ok')

# 3) RolesPage 组件文件
roles_page = '''// W4 设置中心 · 角色(默认角色):system prompt 编辑 + 直通工具清单展示。
// 存 config/roles.json(ADR-0012 口径);保存后下一回合热生效。
import { useEffect, useState } from "react";
import { api } from "./api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export function RolesPage() {
  const [name, setName] = useState("assistant");
  const [prompt, setPrompt] = useState("");
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.rolesGet
      ? undefined
      : undefined;
    fetch("/admin/roles")
      .then((r) => r.json())
      .then((d) => {
        setName(d.name ?? "assistant");
        setPrompt(d.system_prompt ?? "");
      })
      .catch(() => {});
  }, []);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const r = await api.rolesSet(name, prompt);
      setNotice(r.note);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h2 className="text-[15px] font-semibold">角色</h2>
        <p className="text-muted-foreground text-[12.5px]">
          默认角色的 system prompt;保存后下一回合起生效。
          直通工具(只读类,如联网搜索)对所有会话默认开放,无需审批。
        </p>
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

      <div className="bg-card flex flex-col gap-3 rounded-xl border p-4">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="role-name">角色名称</Label>
          <Input
            id="role-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="assistant"
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="role-prompt">System Prompt</Label>
          <textarea
            id="role-prompt"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder="你是 BoenMind 助理……"
            rows={6}
            className="border-input bg-background focus-visible:ring-ring rounded-md border px-3 py-2 text-[13px] outline-none focus-visible:ring-2"
          />
        </div>
        <Button size="sm" disabled={saving} onClick={() => void save()}>
          保存
        </Button>
      </div>
    </div>
  );
}
'''
io.open('src/w2/RolesPage.tsx', 'w', encoding='utf-8', newline='').write(roles_page)
print('RolesPage written')
