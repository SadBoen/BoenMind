/**
 * 专家预设页（设置架构 §六）：管家派给 APP 的"工作人格"管理。
 * 专家与 subagent 角色同池（~/.boenmind/agents/*.md）——子代理派工与
 * APP 专家读同一批人。预置（coding-* 系列与 default）禁删。
 *
 * 2026-08-16 设计定调：
 * - 专家 = 模板（非实例），同一模板可派多个 Agent（码农一号/二号）；
 * - 名称统一 APP 前缀（coding-architect…），名称即描述（无 description 字段）；
 * - 模型下拉选择（按提供商分组，同聊天页）；工具子集复选；
 * - 记忆桶自动绑定 = 专家名（用户不关心命名），删除专家保留桶；
 * - 扩展子集并入作用域机制（插件/Skills 列表徽标），表单不再出现。
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Pencil, Plus, Trash2, Users } from "lucide-react";
import { toast } from "sonner";
import { api, type ExpertDef } from "@/api/client";
import { useAppStore } from "@/stores/app-store";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

/** 内置工具面（与后端 BuiltinTools::NAMES 对齐；None = 全部） */
const BUILTIN_TOOLS = ["read", "write", "edit", "grep", "find", "ls", "bash"] as const;

export function ExpertsSettings() {
  const { t } = useTranslation();
  const [experts, setExperts] = useState<ExpertDef[]>([]);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState<ExpertDef | null>(null);
  /** 新建模式（null = 关闭对话框） */
  const [creating, setCreating] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setExperts(await api.listExperts());
    } catch (err) {
      toast.error(t("settings.experts.loadFailed", { error: String(err) }));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void load();
  }, [load]);

  const remove = async (expert: ExpertDef) => {
    if (!window.confirm(t("settings.experts.deleteConfirm", { name: expert.name }))) return;
    try {
      await api.deleteExpert(expert.name);
      toast.success(t("settings.experts.deleted", { name: expert.name }));
      await load();
    } catch (err) {
      toast.error(t("settings.experts.deleteFailed", { error: String(err) }));
    }
  };

  return (
    <section className="space-y-5">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="flex items-center gap-2 text-lg font-semibold">
            <Users size={18} />
            {t("settings.experts.title")}
          </h2>
          <p className="mt-1 text-sm text-muted-foreground">{t("settings.experts.desc")}</p>
        </div>
        <Button variant="outline" size="sm" onClick={() => setCreating(true)}>
          <Plus size={14} />
          {t("settings.experts.create")}
        </Button>
      </div>

      {loading ? (
        <p className="text-sm text-muted-foreground">{t("settings.experts.loading")}</p>
      ) : experts.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("settings.experts.empty")}</p>
      ) : (
        <div className="space-y-2">
          {experts.map((expert) => (
            <div key={expert.name} className="flex items-center justify-between gap-3 rounded-md border p-3">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-medium">{expert.name}</span>
                  {expert.builtin ? (
                    <Badge variant="secondary" className="text-[10px]">
                      {t("settings.experts.builtin")}
                    </Badge>
                  ) : (
                    <Badge variant="outline" className="text-[10px] font-normal">
                      {t("settings.experts.custom")}
                    </Badge>
                  )}
                  {expert.model && (
                    <Badge variant="outline" className="text-[10px] font-mono font-normal">
                      {expert.model}
                    </Badge>
                  )}
                </div>
                <p className="mt-0.5 text-[10px] text-muted-foreground">
                  {t("settings.experts.toolsCount", { count: expert.tools?.length ?? 0 })}
                  {" · "}
                  {t("settings.experts.bucketAuto", { bucket: expert.memory ?? expert.name })}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                <Button variant="ghost" size="sm" onClick={() => setEditing(expert)}>
                  <Pencil size={14} />
                  {t("settings.experts.edit")}
                </Button>
                {!expert.builtin && (
                  <Button variant="ghost" size="sm" onClick={() => void remove(expert)}>
                    <Trash2 size={14} />
                  </Button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}

      {(editing || creating) && (
        <ExpertEditDialog
          expert={editing ?? null}
          open
          onClose={() => {
            setEditing(null);
            setCreating(false);
          }}
          onSaved={() => void load()}
        />
      )}
    </section>
  );
}

/** 专家编辑对话框（新建 = expert 为 null） */
function ExpertEditDialog({
  expert,
  open,
  onClose,
  onSaved,
}: {
  expert: ExpertDef | null;
  open: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const config = useAppStore((s) => s.config);
  const [name, setName] = useState(expert?.name ?? "");
  const [model, setModel] = useState(expert?.model ?? "");
  const [tools, setTools] = useState<Set<string>>(new Set(expert?.tools ?? []));
  const [systemPrompt, setSystemPrompt] = useState(expert?.system_prompt ?? "");
  const [saving, setSaving] = useState(false);

  /** 模型选项按提供商分组（providerId::modelId），同聊天页模型选择器 */
  const modelGroups = useMemo(() => {
    if (!config) return [];
    return config.providers.map((p) => ({
      id: p.id,
      name: p.name,
      models: p.models.map((m) => ({ value: `${p.id}::${m}`, label: m })),
    }));
  }, [config]);

  const toggleTool = (tool: string, checked: boolean) => {
    setTools((prev) => {
      const next = new Set(prev);
      if (checked) next.add(tool);
      else next.delete(tool);
      return next;
    });
  };

  const save = async () => {
    if (!name.trim()) {
      toast.error(t("settings.experts.nameRequired"));
      return;
    }
    setSaving(true);
    try {
      await api.putExpert(name.trim(), {
        description: "",
        model: model || undefined,
        reasoning: expert?.reasoning,
        tools: tools.size > 0 ? [...tools] : undefined,
        extensions: undefined,
        memory: undefined, // 自动绑定 = 专家名（后端回填）
        system_prompt: systemPrompt,
      });
      toast.success(expert ? t("settings.experts.updated") : t("settings.experts.created"));
      onClose();
      onSaved();
    } catch (err) {
      toast.error(t("settings.experts.saveFailed", { error: String(err) }));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      {/* 黄金比例宽度（页宽 768 × 0.618 ≈ 29.6rem）——2026-08-16 用户定调 */}
      <DialogContent className="max-h-[85vh] overflow-y-auto" size="md">
        <DialogHeader>
          <DialogTitle>
            {expert ? t("settings.experts.editTitle", { name: expert.name }) : t("settings.experts.createTitle")}
          </DialogTitle>
          <DialogDescription>{t("settings.experts.editDesc")}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label>{t("settings.experts.name")}</Label>
            <Input
              value={name}
              disabled={!!expert}
              placeholder="coding-architect"
              onChange={(e) => setName(e.target.value)}
            />
            <p className="text-xs text-muted-foreground">{t("settings.experts.nameHint")}</p>
          </div>
          <div className="space-y-1.5">
            <Label>{t("settings.experts.model")}</Label>
            <Select value={model || "none"} onValueChange={(v) => setModel(!v || v === "none" ? "" : v)}>
              <SelectTrigger className="w-full">
                <SelectValue placeholder={t("settings.experts.modelNone")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="none">{t("settings.experts.modelNone")}</SelectItem>
                {modelGroups.map((group) => (
                  <SelectGroup key={group.id}>
                    <SelectLabel>{group.name}</SelectLabel>
                    {group.models.map((opt) => (
                      <SelectItem key={opt.value} value={opt.value}>
                        {opt.label}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">{t("settings.experts.modelHint")}</p>
          </div>
          <div className="space-y-1.5">
            <Label>{t("settings.experts.tools")}</Label>
            <div className="grid grid-cols-2 gap-1.5 rounded-md border p-3 sm:grid-cols-3">
              {BUILTIN_TOOLS.map((tool) => (
                <label
                  key={tool}
                  className="flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-sm hover:bg-accent/50"
                >
                  <input
                    type="checkbox"
                    checked={tools.has(tool)}
                    onChange={(e) => toggleTool(tool, e.target.checked)}
                    className="size-4 shrink-0 accent-primary"
                  />
                  <span className="font-mono text-xs">{tool}</span>
                </label>
              ))}
            </div>
            <p className="text-xs text-muted-foreground">{t("settings.experts.toolsHint")}</p>
          </div>
          <div className="space-y-1.5">
            <Label>{t("settings.experts.systemPrompt")}</Label>
            <Textarea
              rows={8}
              value={systemPrompt}
              placeholder={t("settings.experts.systemPromptPlaceholder")}
              onChange={(e) => setSystemPrompt(e.target.value)}
            />
          </div>
        </div>
        <DialogFooter>
          <Button onClick={() => void save()} disabled={saving}>
            {saving && <Loader2 size={14} className="animate-spin" />}
            {t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
