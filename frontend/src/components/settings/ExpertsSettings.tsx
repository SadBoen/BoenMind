/**
 * 专家预设页（设置架构 §六）：管家派给 APP 的"工作人格"管理。
 * 专家与 subagent 角色同池（~/.boenmind/agents/*.md）——子代理派工与
 * APP 专家读同一批人。预置（default/architect/coder/reviewer）禁删。
 *
 * 专家 = 角色提示词 + 模型 + 工具子集 + 扩展子集 + 记忆桶。
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Pencil, Plus, Trash2, Users } from "lucide-react";
import { toast } from "sonner";
import { api, type ExpertDef } from "@/api/client";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScopeBadge } from "./ScopePicker";

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
                  <ScopeBadge scopes={undefined} />
                </div>
                <p className="mt-0.5 line-clamp-2 text-xs text-muted-foreground">{expert.description}</p>
                <p className="mt-0.5 text-[10px] text-muted-foreground">
                  {t("settings.experts.toolsCount", { count: expert.tools?.length ?? 0 })}
                  {expert.extensions?.length ? ` · ${t("settings.experts.extCount", { count: expert.extensions.length })}` : ""}
                  {expert.memory ? ` · ${t("settings.experts.memory", { bucket: expert.memory })}` : ""}
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
  const [name, setName] = useState(expert?.name ?? "");
  const [description, setDescription] = useState(expert?.description ?? "");
  const [model, setModel] = useState(expert?.model ?? "");
  const [tools, setTools] = useState((expert?.tools ?? []).join(","));
  const [extensions, setExtensions] = useState((expert?.extensions ?? []).join(","));
  const [memory, setMemory] = useState(expert?.memory ?? "");
  const [systemPrompt, setSystemPrompt] = useState(expert?.system_prompt ?? "");
  const [saving, setSaving] = useState(false);

  const csv = (s: string): string[] => s.split(",").map((x) => x.trim()).filter(Boolean);

  const save = async () => {
    if (!name.trim()) {
      toast.error(t("settings.experts.nameRequired"));
      return;
    }
    setSaving(true);
    try {
      await api.putExpert(name.trim(), {
        description: description.trim(),
        model: model.trim() || undefined,
        reasoning: expert?.reasoning,
        tools: csv(tools),
        extensions: csv(extensions),
        memory: memory.trim() || undefined,
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
      <DialogContent className="max-h-[85vh] overflow-y-auto">
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
              placeholder="architect"
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <div className="space-y-1.5">
            <Label>{t("settings.experts.description")}</Label>
            <Input value={description} onChange={(e) => setDescription(e.target.value)} />
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label>{t("settings.experts.model")}</Label>
              <Input
                value={model}
                placeholder="provider::model（留空 = 跟随默认）"
                onChange={(e) => setModel(e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label>{t("settings.experts.memory")}</Label>
              <Input
                value={memory}
                placeholder="project"
                onChange={(e) => setMemory(e.target.value)}
              />
            </div>
          </div>
          <div className="space-y-1.5">
            <Label>{t("settings.experts.tools")}</Label>
            <Input
              value={tools}
              placeholder="read,bash,edit,write"
              onChange={(e) => setTools(e.target.value)}
            />
            <p className="text-xs text-muted-foreground">{t("settings.experts.toolsHint")}</p>
          </div>
          <div className="space-y-1.5">
            <Label>{t("settings.experts.extensions")}</Label>
            <Input
              value={extensions}
              placeholder="web-search（留空 = 不限制）"
              onChange={(e) => setExtensions(e.target.value)}
            />
            <p className="text-xs text-muted-foreground">{t("settings.experts.extensionsHint")}</p>
          </div>
          <div className="space-y-1.5">
            <Label>{t("settings.experts.systemPrompt")}</Label>
            <Textarea
              rows={8}
              value={systemPrompt}
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
