/**
 * WIKI 中栏阅读器：节点元数据头 + Markdown 正文（prose 排版）。
 * Page 显示不可变标记（修订走 patches）；List/Report/Entity 可编辑按钮。
 */
import { useTranslation } from "react-i18next";
import { GitFork, Link2, Lock, PencilLine, Users } from "lucide-react";
import type { WikiNode } from "@/api/client";
import { Button } from "@/components/ui/button";
import { Markdown } from "@/components/shared/Markdown";
import { LayerBadge } from "./WikiApp";

export function WikiReader({
  node,
  onEdit,
}: {
  node: WikiNode;
  onEdit: () => void;
}) {
  const { t } = useTranslation();
  const immutable = node.layer === "Page";

  return (
    <div className="flex h-full flex-col overflow-y-auto">
      {/* 元数据头 */}
      <div className="flex items-start gap-2 border-b px-5 py-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <LayerBadge layer={node.layer} />
            {immutable ? (
              <span className="flex items-center gap-1 text-[10px] text-muted-foreground" title={t("wiki.pageImmutableHint")}>
                <Lock size={10} />
                {t("wiki.pageImmutable")}
              </span>
            ) : (
              <span className="flex items-center gap-1 text-[10px] text-emerald-600/80">
                <PencilLine size={10} />
                {t("wiki.editable")}
              </span>
            )}
            <span className="ml-auto font-mono text-[10px] text-muted-foreground/70">{node.uid}</span>
          </div>
          <h1 className="mt-1.5 text-lg font-semibold leading-snug">{node.title}</h1>
          {node.node_path && (
            <p className="mt-0.5 text-xs text-muted-foreground">{node.node_path}</p>
          )}
          <div className="mt-1.5 flex flex-wrap items-center gap-3 text-[11px] text-muted-foreground">
            <span>
              {t("wiki.createdAt")} {new Date(node.created_at * 1000).toLocaleString()}
            </span>
            {node.parent_uid && (
              <span className="flex items-center gap-1">
                <GitFork size={11} />
                {t("wiki.partOfChain")} {node.parent_uid}
              </span>
            )}
            {node.raw_path && (
              <span className="flex items-center gap-1">
                <Link2 size={11} />
                {node.raw_path}
              </span>
            )}
          </div>
        </div>
        {!immutable && (
          <Button variant="outline" size="sm" className="h-7 shrink-0 gap-1 px-2.5 text-xs" onClick={onEdit}>
            <PencilLine size={12} />
            {t("wiki.edit")}
          </Button>
        )}
      </div>

      {/* 特殊区：List 成员 / Report 证据链 */}
      {node.layer === "List" && node.members.length > 0 && (
        <div className="flex items-center gap-1.5 border-b bg-muted/30 px-5 py-2 text-xs">
          <Users size={12} className="text-emerald-500/80" />
          <span className="text-muted-foreground">{t("wiki.listMembers")}:</span>
          {node.members.map((m) => (
            <span key={m.uid} className="rounded bg-emerald-500/10 px-1.5 py-0.5 font-mono text-[10px]">
              {m.uid}
            </span>
          ))}
        </div>
      )}
      {node.layer === "Report" && node.references.length > 0 && (
        <div className="flex items-center gap-1.5 border-b bg-muted/30 px-5 py-2 text-xs">
          <Link2 size={12} className="text-amber-500/80" />
          <span className="text-muted-foreground">{t("wiki.evidence")}:</span>
          {node.references.map((r) => (
            <span key={r.ref_uid} className="rounded bg-amber-500/10 px-1.5 py-0.5 font-mono text-[10px]">
              {r.ref_uid}
            </span>
          ))}
        </div>
      )}

      {/* 正文 */}
      <article className="prose prose-sm dark:prose-invert max-w-none flex-1 px-5 py-4">
        <Markdown content={node.body} />
      </article>
    </div>
  );
}
