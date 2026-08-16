/**
 * WIKI 编辑器：新建四类节点 / 编辑学习层（Page 不可变——编辑只对 List/
 * Report/Entity；Page 修订走 patches，M1 无 GUI 入口）。
 * textarea + 编辑/预览切换（骨架参考 coding/Editor.tsx）。
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Eye, Loader2, PencilLine, X } from "lucide-react";
import type { WikiNode } from "@/api/client";
import { api } from "@/api/client";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { toast } from "sonner";
import { Markdown } from "@/components/shared/Markdown";
import { LayerBadge } from "./WikiApp";

export type EditorLayer = "Page" | "List" | "Report" | "Entity";

export interface EditorDraft {
  layer: EditorLayer;
  title: string;
  body: string;
  node_path: string;
  /** Page 文件导入（md/txt 绝对路径） */
  file?: string;
  /** List 成员 UID（逗号分隔） */
  members: string;
  /** Report 证据 UID（逗号分隔） */
  references: string;
  /** Entity 源 Page UID */
  source_page: string;
}

export function WikiEditor({
  layer,
  initial,
  onCancel,
  onSaved,
}: {
  layer: EditorLayer;
  initial?: WikiNode;
  onCancel: () => void;
  onSaved: (node: WikiNode) => void;
}) {
  const { t } = useTranslation();
  const editing = !!initial;
  const [draft, setDraft] = useState<EditorDraft>({
    layer,
    title: initial?.title ?? "",
    body: initial?.body ?? "",
    node_path: initial?.node_path ?? "",
    file: undefined,
    members: initial?.members.map((m) => m.uid).join(", ") ?? "",
    references: initial?.references.map((r) => r.ref_uid).join(", ") ?? "",
    source_page: initial?.source_page ?? "",
  });
  const [preview, setPreview] = useState(false);
  const [saving, setSaving] = useState(false);

  const set = <K extends keyof EditorDraft>(key: K, value: EditorDraft[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));

  const save = async () => {
    if (!draft.title.trim()) {
      toast.error(t("wiki.editor.titleRequired"));
      return;
    }
    if (!draft.body.trim()) {
      toast.error(t("wiki.editor.bodyRequired"));
      return;
    }
    setSaving(true);
    try {
      let node: WikiNode;
      if (editing && initial) {
        node = await api.wikiUpdateNode(initial.uid, {
          title: draft.title,
          body: draft.body,
        });
        toast.success(t("wiki.editor.saved"));
      } else if (layer === "Page") {
        const res = await api.wikiIngest({
          title: draft.title,
          content: draft.body,
          node_path: draft.node_path,
          file: draft.file,
        });
        const first = res.pages[0];
        if (!first) {
          toast.error(t("wiki.editor.noPage"));
          setSaving(false);
          return;
        }
        node = await api.wikiNode(first.uid);
        toast.success(
          res.pages.length > 1
            ? t("wiki.editor.pagesCreated", { n: res.pages.length })
            : t("wiki.editor.saved"),
        );
      } else if (layer === "List") {
        node = await api.wikiCreateList({
          title: draft.title,
          body: draft.body,
          node_path: draft.node_path,
          members: draft.members
            .split(",")
            .map((s) => s.trim())
            .filter(Boolean),
        });
        toast.success(t("wiki.editor.saved"));
      } else if (layer === "Report") {
        node = await api.wikiCreateReport({
          title: draft.title,
          body: draft.body,
          node_path: draft.node_path,
          references: draft.references
            .split(",")
            .map((s) => s.trim())
            .filter(Boolean)
            .map((ref_uid) => ({ ref_uid })),
        });
        toast.success(t("wiki.editor.saved"));
      } else {
        node = await api.wikiCreateEntity({
          title: draft.title,
          body: draft.body,
          node_path: draft.node_path,
          source_page: draft.source_page.trim() || undefined,
        });
        toast.success(t("wiki.editor.saved"));
      }
      onSaved(node);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* 头部 */}
      <div className="flex items-center gap-2 border-b px-4 py-2.5">
        <LayerBadge layer={layer} />
        <span className="text-sm font-medium">
          {editing ? t("wiki.editor.editTitle") : t("wiki.editor.newTitle")}
        </span>
        <div className="flex-1" />
        <Button
          variant="ghost"
          size="sm"
          className="h-7 gap-1 px-2 text-xs"
          onClick={() => setPreview((p) => !p)}
        >
          {preview ? <PencilLine size={12} /> : <Eye size={12} />}
          {preview ? t("wiki.editor.write") : t("wiki.editor.preview")}
        </Button>
        <Button variant="ghost" size="sm" className="h-7 px-2" onClick={onCancel}>
          <X size={14} />
        </Button>
      </div>

      {/* 表单 */}
      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        <div className="flex flex-col gap-3">
          <div className="grid grid-cols-2 gap-2">
            <div className="col-span-2">
              <Label className="text-xs">{t("wiki.editor.title")}</Label>
              <Input
                value={draft.title}
                onChange={(e) => set("title", e.target.value)}
                className="mt-1 h-8 text-sm"
                placeholder={t("wiki.editor.titlePh")}
              />
            </div>
            <div>
              <Label className="text-xs">{t("wiki.editor.nodePath")}</Label>
              <Input
                value={draft.node_path}
                onChange={(e) => set("node_path", e.target.value)}
                className="mt-1 h-8 text-sm"
                placeholder="papers/ml"
              />
            </div>
            {layer === "Page" && !editing && (
              <div>
                <Label className="text-xs">{t("wiki.editor.file")}</Label>
                <Input
                  value={draft.file ?? ""}
                  onChange={(e) => set("file", e.target.value || undefined)}
                  className="mt-1 h-8 text-sm"
                  placeholder="C:\\path\\note.md"
                />
              </div>
            )}
            {layer === "List" && (
              <div className="col-span-2">
                <Label className="text-xs">{t("wiki.editor.members")}</Label>
                <Input
                  value={draft.members}
                  onChange={(e) => set("members", e.target.value)}
                  className="mt-1 h-8 text-sm"
                  placeholder="UID1, UID2"
                />
              </div>
            )}
            {layer === "Report" && (
              <div className="col-span-2">
                <Label className="text-xs">{t("wiki.editor.evidence")}</Label>
                <Input
                  value={draft.references}
                  onChange={(e) => set("references", e.target.value)}
                  className="mt-1 h-8 text-sm"
                  placeholder="UID1, UID2"
                />
              </div>
            )}
            {layer === "Entity" && (
              <div className="col-span-2">
                <Label className="text-xs">{t("wiki.editor.sourcePage")}</Label>
                <Input
                  value={draft.source_page}
                  onChange={(e) => set("source_page", e.target.value)}
                  className="mt-1 h-8 text-sm"
                  placeholder="UID"
                />
              </div>
            )}
          </div>

          <div>
            <Label className="text-xs">{t("wiki.editor.body")}</Label>
            {preview ? (
              <article className="prose prose-sm dark:prose-invert max-w-none mt-1 min-h-[16rem] rounded-lg border bg-muted/20 px-4 py-3">
                <Markdown content={draft.body} />
              </article>
            ) : (
              <Textarea
                value={draft.body}
                onChange={(e) => set("body", e.target.value)}
                className="mt-1 min-h-[16rem] font-mono text-xs leading-relaxed"
                placeholder="Markdown…"
              />
            )}
          </div>
        </div>
      </div>

      {/* 底部操作 */}
      <div className="flex items-center justify-end gap-2 border-t px-4 py-2.5">
        <Button variant="ghost" size="sm" className="h-8 px-3 text-xs" onClick={onCancel}>
          {t("wiki.editor.cancel")}
        </Button>
        <Button size="sm" className="h-8 gap-1 px-4 text-xs" onClick={() => void save()} disabled={saving}>
          {saving && <Loader2 size={12} className="animate-spin" />}
          {t("wiki.editor.save")}
        </Button>
      </div>
    </div>
  );
}
