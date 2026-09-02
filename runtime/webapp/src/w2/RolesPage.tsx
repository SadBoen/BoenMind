// W4b 设置中心 · 角色管理:多角色列表 + 新增/编辑 + 设为默认。
// 存 config/roles.json(ADR-0012 口径);保存后会话与下一回合热生效。
import { useEffect, useState } from "react";
import { api, type RoleItem } from "./api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { CheckIcon, PlusIcon, Trash2Icon, Edit3Icon } from "lucide-react";

export function RolesPage() {
  const [roles, setRoles] = useState<RoleItem[]>([]);
  const [activeId, setActiveId] = useState<string>("assistant");
  const [editingRole, setEditingRole] = useState<RoleItem | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = async () => {
    try {
      const d = await api.roles.get();
      setRoles(d.roles);
      setActiveId(d.active_id);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  const openNew = () => {
    setIsNew(true);
    setEditingRole({
      id: "role_" + Math.random().toString(36).substring(2, 7),
      name: "",
      description: "",
      system_prompt: "",
    });
  };

  const openEdit = (r: RoleItem) => {
    setIsNew(false);
    setEditingRole({ ...r });
  };

  const saveRole = async () => {
    if (!editingRole) return;
    if (!editingRole.name.trim()) {
      setError("角色名称不能为空");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const res = await api.roles.save(editingRole);
      setNotice(res.note);
      setEditingRole(null);
      await reload();
      window.dispatchEvent(new CustomEvent("bm-roles-changed"));
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setSaving(false);
    }
  };

  const deleteRole = async (id: string) => {
    if (!window.confirm("确定删除该角色吗？")) return;
    try {
      const res = await api.roles.delete(id);
      setNotice(res.note);
      await reload();
      window.dispatchEvent(new CustomEvent("bm-roles-changed"));
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  const makeActive = async (id: string) => {
    try {
      const res = await api.roles.setActive(id);
      setActiveId(res.active_id);
      setNotice(res.note);
      window.dispatchEvent(new CustomEvent("bm-roles-changed"));
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-[15px] font-semibold">角色管理</h2>
          <p className="text-muted-foreground text-[12.5px]">
            管理不同的系统提示词与角色预设。可设默认角色，也可在聊天窗口随开随切。
          </p>
        </div>
        <Button size="sm" onClick={openNew} data-slot="add-role-btn">
          <PlusIcon className="mr-1 size-3.5" />
          新建角色
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

      {editingRole ? (
        <div className="bg-card flex flex-col gap-3 rounded-xl border p-4 shadow-sm" data-slot="role-edit-form">
          <div className="text-[14px] font-medium">
            {isNew ? "新建角色" : `编辑角色: ${editingRole.name}`}
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="role-name">角色名称</Label>
              <Input
                id="role-name"
                value={editingRole.name}
                onChange={(e) =>
                  setEditingRole({ ...editingRole, name: e.target.value })
                }
                placeholder="例如: 编程专家、翻译助手..."
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="role-desc">简短描述 (可选)</Label>
              <Input
                id="role-desc"
                value={editingRole.description ?? ""}
                onChange={(e) =>
                  setEditingRole({ ...editingRole, description: e.target.value })
                }
                placeholder="一句话介绍该角色的专长"
              />
            </div>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="role-prompt">System Prompt</Label>
            <textarea
              id="role-prompt"
              value={editingRole.system_prompt}
              onChange={(e) =>
                setEditingRole({ ...editingRole, system_prompt: e.target.value })
              }
              placeholder="你是一位经验丰富的全栈开发专家……"
              rows={5}
              className="border-input bg-background focus-visible:ring-ring rounded-md border px-3 py-2 font-mono text-[12.5px] outline-none focus-visible:ring-2"
            />
          </div>
          <div className="flex justify-end gap-2 pt-1">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setEditingRole(null)}
            >
              取消
            </Button>
            <Button size="sm" disabled={saving} onClick={() => void saveRole()}>
              {saving ? "保存中..." : "保存角色"}
            </Button>
          </div>
        </div>
      ) : null}

      <div className="flex flex-col gap-2.5">
        {roles.map((r) => {
          const isDefault = r.id === activeId;
          return (
            <div
              key={r.id}
              className="bg-card flex items-start justify-between rounded-xl border p-3.5 transition-colors"
              data-role-id={r.id}
            >
              <div className="min-w-0 flex-1 pr-3">
                <div className="flex items-center gap-2">
                  <span className="font-semibold text-[13.5px]">{r.name}</span>
                  {isDefault ? (
                    <Badge className="bg-primary/90 text-[10px]">全局默认</Badge>
                  ) : null}
                  <span className="font-mono text-[11px] text-muted-foreground">
                    ID: {r.id}
                  </span>
                </div>
                {r.description ? (
                  <div className="text-muted-foreground mt-0.5 text-[12px]">
                    {r.description}
                  </div>
                ) : null}
                <div className="bg-muted/40 text-muted-foreground mt-2 line-clamp-2 rounded-md px-2.5 py-1.5 font-mono text-[11.5px]">
                  {r.system_prompt || "(无 System Prompt)"}
                </div>
              </div>

              <div className="flex items-center gap-1.5 shrink-0 pt-0.5">
                {!isDefault ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="text-[12px] h-8 text-muted-foreground"
                    title="设为全局默认角色"
                    onClick={() => void makeActive(r.id)}
                  >
                    <CheckIcon className="mr-1 size-3.5" />
                    设为默认
                  </Button>
                ) : null}
                <Button
                  variant="ghost"
                  size="sm"
                  className="size-8 p-0"
                  title="编辑角色"
                  onClick={() => openEdit(r)}
                >
                  <Edit3Icon className="size-3.5" />
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  className="size-8 p-0 text-red-600 hover:text-red-700"
                  disabled={roles.length <= 1}
                  title={roles.length <= 1 ? "至少保留一个角色" : "删除角色"}
                  onClick={() => void deleteRole(r.id)}
                >
                  <Trash2Icon className="size-3.5" />
                </Button>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
