/**
 * WIKI 左栏：检索框 + 四分区节点树（Pages/Lists/Reports/Entities）。
 * 分区折叠本地记忆（useState 默认展开 Pages）；检索回车触发中栏检索视图。
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { BookMarked, ChevronDown, ChevronRight, FileText, GitFork, ListPlus, Loader2, Search } from "lucide-react";
import { cn } from "@/lib/utils";
import type { WikiTree, WikiTreeEntry } from "@/api/client";
import { Input } from "@/components/ui/input";
import { LayerBadge } from "./WikiApp";

const SECTION_ICON = {
  pages: <FileText size={13} className="text-blue-500/80" />,
  lists: <ListPlus size={13} className="text-emerald-500/80" />,
  reports: <GitFork size={13} className="text-amber-500/80" />,
  entities: <BookMarked size={13} className="text-purple-500/80" />,
} as const;

type SectionKey = keyof typeof SECTION_ICON;

export function WikiTreePanel({
  tree,
  onSearch,
  onOpen,
  activeUid,
}: {
  tree: WikiTree | null;
  onSearch: (keywords: string) => void;
  onOpen: (uid: string) => void;
  activeUid: string | null;
}) {
  const { t } = useTranslation();
  const [kw, setKw] = useState("");
  const [searching, setSearching] = useState(false);
  const [collapsed, setCollapsed] = useState<Partial<Record<SectionKey, boolean>>>({
    lists: true,
    reports: true,
    entities: true,
  });

  const runSearch = async () => {
    if (!kw.trim()) return;
    setSearching(true);
    await onSearch(kw);
    setSearching(false);
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* 检索框 */}
      <div className="shrink-0 p-2.5">
        <div className="relative">
          <Search size={13} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={kw}
            onChange={(e) => setKw(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void runSearch();
            }}
            placeholder={t("wiki.searchPlaceholder")}
            className="h-8 pl-8 pr-7 text-xs"
          />
          {searching && (
            <Loader2 size={13} className="absolute right-2.5 top-1/2 -translate-y-1/2 animate-spin text-muted-foreground" />
          )}
        </div>
      </div>

      {/* 节点树 */}
      <div className="min-h-0 flex-1 overflow-y-auto pb-4">
        {!tree ? (
          <div className="flex justify-center py-8">
            <Loader2 size={16} className="animate-spin text-muted-foreground" />
          </div>
        ) : (
          <div className="flex flex-col gap-0.5 px-1.5">
            {(["pages", "lists", "reports", "entities"] as SectionKey[]).map((key) => {
              const entries = tree[key];
              const isCollapsed = collapsed[key];
              return (
                <div key={key}>
                  <button
                    className="flex w-full items-center gap-1 rounded px-1.5 py-1 text-xs font-medium text-muted-foreground hover:bg-accent/50"
                    onClick={() => setCollapsed((c) => ({ ...c, [key]: !c[key] }))}
                  >
                    {isCollapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
                    {SECTION_ICON[key]}
                    <span>{t(`wiki.sections.${key}`)}</span>
                    <span className="ml-auto text-[10px] tabular-nums">{entries.length}</span>
                  </button>
                  {!isCollapsed && (
                    <div className="flex flex-col">
                      {entries.length === 0 && (
                        <p className="px-6 py-1 text-[11px] text-muted-foreground/60">
                          {t("wiki.sectionEmpty")}
                        </p>
                      )}
                      {entries.map((e) => (
                        <TreeItem key={e.uid} entry={e} active={e.uid === activeUid} onOpen={onOpen} />
                      ))}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

function TreeItem({
  entry,
  active,
  onOpen,
}: {
  entry: WikiTreeEntry;
  active: boolean;
  onOpen: (uid: string) => void;
}) {
  return (
    <button
      className={cn(
        "flex w-full items-center gap-1.5 rounded px-3 py-1 text-left text-xs",
        active ? "bg-accent text-foreground" : "text-muted-foreground hover:bg-accent/40",
      )}
      onClick={() => onOpen(entry.uid)}
      title={`${entry.title} · ${entry.uid}`}
    >
      <span className="truncate">{entry.title}</span>
      {entry.node_path && <span className="ml-auto shrink-0 text-[10px] text-muted-foreground/60">{entry.node_path}</span>}
    </button>
  );
}

// LayerBadge 供树内展示复用（当前树项仅标题；徽标在阅读视图显示）
export { LayerBadge as WikiLayerBadge };
