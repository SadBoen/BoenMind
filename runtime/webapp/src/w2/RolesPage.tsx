// W4b 设置中心 · 角色管理:多角色列表 + 新增/编辑 + 设为默认 + 技能库挂载。
// 存 config/roles.json 与 config/skills.json(ADR-0012 口径;合同
// capability/skill.v0_1);保存后下一回合热生效。Skill 只是数据,加载不改变权限。
import { useEffect, useState } from "react";
import { api, type RoleItem, type SkillItem } from "./api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  CheckIcon,
  PlusIcon,
  Trash2Icon,
  Edit3Icon,
  BookOpenIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";

export function RolesPage() {
  const [roles, setRoles] = useState<RoleItem[]>([]);
  const [activeId, setActiveId] = useState<string>("assistant");
  const [skills, setSkills] = useState<SkillItem[]>([]);
  const [editingRole, setEditingRole] = useState<RoleItem | null>(null);
  const [editingSkill, setEditingSkill] = useState<SkillItem | null>(null);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = async () => {
    try {
      const [r, s] = await Promise.all([api.roles.get(), api.skills.list()]);
      setRoles(r.roles);
      setActiveId(r.active_id);
      setSkills(s.skills);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  const flash = (msg: string | null) => {
    setNotice(msg);
    if (msg) setTimeout(() => setNotice(null), 5000);
  };

  // ---- 角色编辑 -----------------------------------------------------------
  const openNewRole = () => {
    setEditingRole({
      id: "role_" + Math.random().toString(36).substring(2, 7),
      name: "",
      description: "",
      system_prompt: "",
      skills: [],
    });
  };

  const openEditRole = (r: RoleItem) => {
    setEditingRole({ ...r, skills: r.skills ?? [] });
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
      flash(res.note);
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
      flash(res.note);
      await reload();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  const makeActive = async (id: string) => {
    try {
      const res = await api.roles.setActive(id);
      setActiveId(res.active_id);
      flash(res.note);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  // ---- 技能编辑 -----------------------------------------------------------
  const openNewSkill = () => {
    setEditingSkill({
      skill_id:
        "skill_" + Math.random().toString(36).substring(2, 8),
      name: "",
      description: "",
      instruction: "",
      allowed_capabilities: [],
    });
  };

  const saveSkill = async () => {
    if (!editingSkill) return;
    if (!editingSkill.name.trim() || !editingSkill.instruction.trim()) {
      setError("技能名称与指令正文不能为空");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const res = await api.skills.save(editingSkill);
      flash(res.note);
      setEditingSkill(null);
      await reload();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setSaving(false);
    }
  };

  const deleteSkill = async (id: string) => {
    if (!window.confirm("确定删除该技能吗?(已挂载的角色将自动跳过)")) return;
    try {
      const res = await api.skills.remove(id);
      flash(res.note);
      await reload();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  const toggleSkillMount = (skillId: string) => {
    if (!editingRole) return;
    const cur = editingRole.skills ?? [];
    const next = cur.includes(skillId)
      ? cur.filter((s) => s !== skillId)
      : [...cur, skillId];
    setEditingRole({ ...editingRole, skills: next });
  };

  const skillName = (id: string) =>
    skills.find((s) => s.skill_id === id)?.name ?? id;

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h2 className="text-[15px] font-semibold">角色与技能</h2>
        <p className="text-muted-foreground text-[12.5px]">
          管理角色预设与技能知识包;技能挂载到角色后,其指令会追加进 system
          prompt。技能只是数据,加载不改变工具权限。
        </p>
      </div>

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

      {/* ---- 角色编辑表单 ---- */}
      {editingRole ? (
        <div
          className="bg-card flex flex-col gap-3 rounded-xl border p-4 shadow-sm"
          data-slot="role-edit-form"
        >
          <div className="text-[14px] font-medium">
            {roles.some((r) => r.id === editingRole.id)
              ? `编辑角色: ${editingRole.name}`
              : "新建角色"}
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
                  setEditingRole({
                    ...editingRole,
                    description: e.target.value,
                  })
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
                setEditingRole({
                  ...editingRole,
                  system_prompt: e.target.value,
                })
              }
              placeholder="你是一位经验丰富的全栈开发专家……"
              rows={5}
              className="border-input bg-background focus-visible:ring-ring rounded-md border px-3 py-2 font-mono text-[12.5px] outline-none focus-visible:ring-2"
            />
          </div>
          {skills.length > 0 ? (
            <div className="flex flex-col gap-1.5">
              <Label>挂载技能(指令追加进 system prompt)</Label>
              <div className="flex flex-wrap gap-2">
                {skills.map((s) => {
                  const mounted = (editingRole.skills ?? []).includes(
                    s.skill_id,
                  );
                  return (
                    <button
                      key={s.skill_id}
                      type="button"
                      title={s.description ?? s.instruction.slice(0, 80)}
                      className={cn(
                        "flex items-center gap-1 rounded-full border px-2.5 py-1 text-[12px] transition-colors",
                        mounted
                          ? "bg-primary text-primary-foreground border-primary"
                          : "text-muted-foreground hover:bg-muted",
                      )}
                      data-slot="skill-mount"
                      data-skill-id={s.skill_id}
                      data-mounted={mounted}
                      onClick={() => toggleSkillMount(s.skill_id)}
                    >
                      {mounted ? <CheckIcon className="size-3" /> : <PlusIcon className="size-3" />}
                      {s.name}
                    </button>
                  );
                })}
              </div>
            </div>
          ) : null}
          <div className="flex justify-end gap-2 pt-1">
            <Button variant="outline" size="sm" onClick={() => setEditingRole(null)}>
              取消
            </Button>
            <Button size="sm" disabled={saving} onClick={() => void saveRole()}>
              {saving ? "保存中..." : "保存角色"}
            </Button>
          </div>
        </div>
      ) : null}

      {/* ---- 角色列表 ---- */}
      <div className="flex flex-col gap-2.5">
        <div className="flex items-center justify-between">
          <span className="text-[13.5px] font-semibold">角色</span>
          <Button size="sm" onClick={openNewRole} data-slot="add-role-btn">
            <PlusIcon className="mr-1 size-3.5" />
            新建角色
          </Button>
        </div>
        {roles.map((r) => {
          const isDefault = r.id === activeId;
          return (
            <div
              key={r.id}
              className="bg-card flex items-start justify-between rounded-xl border p-3.5"
              data-role-id={r.id}
            >
              <div className="min-w-0 flex-1 pr-3">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-[13.5px] font-semibold">{r.name}</span>
                  {isDefault ? (
                    <Badge className="bg-primary/90 text-[10px]">全局默认</Badge>
                  ) : null}
                  <span className="text-muted-foreground font-mono text-[11px]">
                    ID: {r.id}
                  </span>
                </div>
                {r.description ? (
                  <div className="text-muted-foreground mt-0.5 text-[12px]">
                    {r.description}
                  </div>
                ) : null}
                {(r.skills ?? []).length > 0 ? (
                  <div className="mt-1 flex flex-wrap gap-1">
                    {(r.skills ?? []).map((sid) => (
                      <Badge key={sid} variant="outline" className="text-[10px]">
                        <BookOpenIcon className="mr-1 size-2.5" />
                        {skillName(sid)}
                      </Badge>
                    ))}
                  </div>
                ) : null}
                <div className="bg-muted/40 text-muted-foreground mt-2 line-clamp-2 rounded-md px-2.5 py-1.5 font-mono text-[11.5px]">
                  {r.system_prompt || "(无 System Prompt)"}
                </div>
              </div>

              <div className="flex shrink-0 items-center gap-1.5 pt-0.5">
                {!isDefault ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-8 text-[12px] text-muted-foreground"
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
                  onClick={() => openEditRole(r)}
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

      {/* ---- 技能编辑表单 ---- */}
      {editingSkill ? (
        <div
          className="bg-card flex flex-col gap-3 rounded-xl border p-4 shadow-sm"
          data-slot="skill-edit-form"
        >
          <div className="text-[14px] font-medium">
            {skills.some((s) => s.skill_id === editingSkill.skill_id)
              ? `编辑技能: ${editingSkill.name}`
              : "新建技能"}
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="skill-name">技能名称</Label>
              <Input
                id="skill-name"
                value={editingSkill.name}
                onChange={(e) =>
                  setEditingSkill({ ...editingSkill, name: e.target.value })
                }
                placeholder="例如: 代码评审规范"
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="skill-desc">简短描述 (可选)</Label>
              <Input
                id="skill-desc"
                value={editingSkill.description ?? ""}
                onChange={(e) =>
                  setEditingSkill({ ...editingSkill, description: e.target.value })
                }
                placeholder="一句话说明该技能的作用"
              />
            </div>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="skill-instruction">技能指令(挂载后追加进 system prompt)</Label>
            <textarea
              id="skill-instruction"
              value={editingSkill.instruction}
              onChange={(e) =>
                setEditingSkill({ ...editingSkill, instruction: e.target.value })
              }
              placeholder="评审代码时请遵循: 1) 先指出风险 2) 再给改进建议……"
              rows={4}
              className="border-input bg-background focus-visible:ring-ring rounded-md border px-3 py-2 font-mono text-[12.5px] outline-none focus-visible:ring-2"
            />
          </div>
          <div className="flex justify-end gap-2 pt-1">
            <Button variant="outline" size="sm" onClick={() => setEditingSkill(null)}>
              取消
            </Button>
            <Button size="sm" disabled={saving} onClick={() => void saveSkill()}>
              {saving ? "保存中..." : "保存技能"}
            </Button>
          </div>
        </div>
      ) : null}

      {/* ---- 技能库列表 ---- */}
      <div className="flex flex-col gap-2.5">
        <div className="flex items-center justify-between">
          <span className="flex items-center gap-1.5 text-[13.5px] font-semibold">
            <BookOpenIcon className="size-4" />
            技能库
          </span>
          <Button size="sm" onClick={openNewSkill} data-slot="add-skill-btn">
            <PlusIcon className="mr-1 size-3.5" />
            新建技能
          </Button>
        </div>
        {skills.length === 0 ? (
          <div className="text-muted-foreground rounded-lg border border-dashed px-3 py-6 text-center text-[12.5px]">
            技能库为空——新建技能并在角色编辑中挂载。
          </div>
        ) : (
          skills.map((s) => (
            <div
              key={s.skill_id}
              className="bg-card flex items-start justify-between rounded-xl border p-3"
              data-skill-id={s.skill_id}
            >
              <div className="min-w-0 flex-1 pr-3">
                <div className="flex items-center gap-2">
                  <span className="text-[13px] font-medium">{s.name}</span>
                  <span className="text-muted-foreground font-mono text-[11px]">
                    {s.skill_id}
                  </span>
                </div>
                {s.description ? (
                  <div className="text-muted-foreground mt-0.5 text-[12px]">
                    {s.description}
                  </div>
                ) : null}
              </div>
              <Button
                variant="ghost"
                size="sm"
                className="size-8 p-0 text-red-600 hover:text-red-700"
                title="删除技能"
                onClick={() => void deleteSkill(s.skill_id)}
              >
                <Trash2Icon className="size-3.5" />
              </Button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
