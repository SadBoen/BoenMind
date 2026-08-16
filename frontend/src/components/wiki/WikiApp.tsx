/**
 * WIKI 应用（xu-wiki 迁移 · bm-wiki 引擎）三栏容器。
 *
 * ┌────────┬──────────────────┬───────────┐
 * │ 左：树  │  中：阅读/编辑/检索 │  右：关系/对话 │
 * └────────┴──────────────────┴───────────┘
 * 库 = working_dir/wiki（三件套 raws/nodes/.xu，格式与 xu-wiki 兼容）。
 * 状态为组件本地（不污染全局 store）：树/活动节点/检索/编辑 全在此层。
 * 右栏对话 = ChatPane panel 形态 scene="wiki"（一软件一会话，ensureAppSession）。
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { BookOpen, Loader2, ListPlus, PencilLine, RefreshCw, Search, FileText, GitFork, BookMarked } from "lucide-react";
import { api, type WikiHit, type WikiNode, type WikiStatus, type WikiTree } from "@/api/client";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";
import { WikiTreePanel } from "./WikiTree";
import { WikiMeta } from "./WikiMeta";
import { WikiReader } from "./WikiReader";
import { WikiEditor, type EditorLayer } from "./WikiEditor";

type CenterView =
  | { kind: "empty" }
  | { kind: "read"; node: WikiNode }
  | { kind: "search"; keywords: string; hits: WikiHit[]; loading: boolean }
  | { kind: "new"; layer: EditorLayer }
  | { kind: "edit"; node: WikiNode };

export function WikiApp() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<WikiStatus | null>(null);
  const [tree, setTree] = useState<WikiTree | null>(null);
  const [center, setCenter] = useState<CenterView>({ kind: "empty" });
  const [refreshing, setRefreshing] = useState(false);

  const loadStatus = useCallback(async () => {
    try {
      const s = await api.wikiStatus();
      setStatus(s);
      return s;
    } catch {
      setStatus(null);
      return null;
    }
  }, []);

  const loadTree = useCallback(async () => {
    try {
      setTree(await api.wikiTree());
    } catch {
      setTree(null);
    }
  }, []);

  useEffect(() => {
    void (async () => {
      const s = await loadStatus();
      if (s?.exists) void loadTree();
    })();
  }, [loadStatus, loadTree]);

  const refresh = useCallback(async () => {
    setRefreshing(true);
    await loadStatus();
    await loadTree();
    setRefreshing(false);
  }, [loadStatus, loadTree]);

  const createWiki = useCallback(async () => {
    try {
      await api.wikiCreate("my-wiki");
      toast.success(t("wiki.created"));
      await loadStatus();
      await loadTree();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }, [loadStatus, loadTree, t]);

  const openNode = useCallback(
    async (uid: string) => {
      try {
        const node = await api.wikiNode(uid);
        setCenter({ kind: "read", node });
      } catch (e) {
        toast.error(e instanceof Error ? e.message : String(e));
      }
    },
    [],
  );

  const onSearch = useCallback(async (keywords: string) => {
    if (!keywords.trim()) return;
    setCenter({ kind: "search", keywords, hits: [], loading: true });
    try {
      const res = await api.wikiQuery(keywords);
      setCenter({ kind: "search", keywords, hits: res.hits, loading: false });
    } catch {
      setCenter({ kind: "search", keywords, hits: [], loading: false });
    }
  }, []);

  /** 节点内容变更后回调（编辑/关系）：刷新 status（总数）+ 树 + 重读活动节点 */
  const refreshActive = useCallback(
    async (uid?: string) => {
      await loadStatus();
      await loadTree();
      if (uid) {
        try {
          const node = await api.wikiNode(uid);
          setCenter({ kind: "read", node });
        } catch {
          /* 节点可能被删除——回空态 */
          setCenter({ kind: "empty" });
        }
      }
    },
    [loadStatus, loadTree],
  );

  // ── 建库引导 ──
  if (status && !status.exists) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4 bg-background">
        <BookOpen size={44} strokeWidth={1.5} className="text-muted-foreground" />
        <p className="max-w-md text-center text-sm text-muted-foreground">{t("wiki.createHint")}</p>
        <Button onClick={() => void createWiki()}>
          <BookOpen size={16} className="mr-1.5" />
          {t("wiki.create")}
        </Button>
      </div>
    );
  }

  const counts = status?.counts;
  const total = counts ? counts.pages + counts.lists + counts.reports + counts.entities : 0;

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      {/* 顶部工具条：库状态 + 新建 + 刷新 */}
      <div className="flex h-10 shrink-0 items-center gap-2 border-b px-3">
        <BookOpen size={15} className="text-muted-foreground" />
        <span className="text-xs font-medium">{t("wiki.title")}</span>
        <span className="text-xs text-muted-foreground">
          {total} {t("wiki.nodes")} · {t("wiki.relationCap")}
        </span>
        <div className="flex-1" />
        <NewNodeMenu onPick={(layer) => setCenter({ kind: "new", layer })} />
        <Button variant="ghost" size="sm" className="h-7 px-2" onClick={() => void refresh()} disabled={refreshing}>
          {refreshing ? <Loader2 size={14} className="animate-spin" /> : <RefreshCw size={14} />}
        </Button>
      </div>

      <div className="flex min-h-0 flex-1">
        {/* 左：树 + 检索 */}
        <div className="flex w-60 shrink-0 flex-col border-r">
          <WikiTreePanel tree={tree} onSearch={onSearch} onOpen={openNode} activeUid={activeUid(center)} />
        </div>
        {/* 中：阅读/编辑/检索 */}
        <div className="min-w-0 flex-1">
          {center.kind === "empty" && (
            <div className="flex h-full flex-col items-center justify-center gap-3 text-muted-foreground">
              <Search size={36} strokeWidth={1.5} />
              <p className="max-w-sm px-6 text-center text-sm">{t("wiki.empty")}</p>
            </div>
          )}
          {center.kind === "read" && (
            <WikiReader node={center.node} onEdit={() => setCenter({ kind: "edit", node: center.node })} />
          )}
          {center.kind === "search" && (
            <WikiSearchResults
              keywords={center.keywords}
              hits={center.hits}
              loading={center.loading}
              onOpen={openNode}
              onBack={() => setCenter({ kind: "empty" })}
            />
          )}
          {(center.kind === "new" || center.kind === "edit") && (
            <WikiEditor
              key={center.kind === "new" ? `new-${center.layer}` : `edit-${center.node.uid}`}
              layer={center.kind === "new" ? center.layer : center.node.layer}
              initial={center.kind === "edit" ? center.node : undefined}
              onCancel={() =>
                setCenter(center.kind === "edit" ? { kind: "read", node: center.node } : { kind: "empty" })
              }
              onSaved={(node) => {
                void refreshActive(node.uid);
              }}
            />
          )}
        </div>
        {/* 右：关系 + 对话 */}
        <div className="flex w-72 shrink-0 flex-col border-l">
          <WikiMeta
            activeUid={activeUid(center)}
            activeNode={center.kind === "read" || center.kind === "edit" ? center.node : undefined}
            onOpenNode={openNode}
            onChanged={refreshActive}
          />
        </div>
      </div>
    </div>
  );
}

function activeUid(center: CenterView): string | null {
  return center.kind === "read" || center.kind === "edit" ? center.node.uid : null;
}

/** 新建节点入口（四类） */
function NewNodeMenu({ onPick }: { onPick: (layer: EditorLayer) => void }) {
  const { t } = useTranslation();
  const items: { layer: EditorLayer; label: string; icon: React.ReactNode }[] = [
    { layer: "Page", label: t("wiki.newPage"), icon: <FileText size={13} /> },
    { layer: "List", label: t("wiki.newList"), icon: <ListPlus size={13} /> },
    { layer: "Report", label: t("wiki.newReport"), icon: <GitFork size={13} /> },
    { layer: "Entity", label: t("wiki.newEntity"), icon: <BookMarked size={13} /> },
  ];
  return (
    <div className="flex items-center gap-1">
      {items.map((it) => (
        <Button key={it.layer} variant="outline" size="sm" className="h-7 gap-1 px-2 text-xs" onClick={() => onPick(it.layer)}>
          {it.icon}
          {it.label}
        </Button>
      ))}
    </div>
  );
}

/** 检索结果视图（中栏） */
function WikiSearchResults({
  keywords,
  hits,
  loading,
  onOpen,
  onBack,
}: {
  keywords: string;
  hits: WikiHit[];
  loading: boolean;
  onOpen: (uid: string) => void;
  onBack: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <div className="flex items-center gap-2 border-b px-4 py-2.5">
        <PencilLine size={14} className="text-muted-foreground" />
        <span className="text-sm font-medium">{t("wiki.searchResults")}</span>
        <span className="text-xs text-muted-foreground">“{keywords}”</span>
        <div className="flex-1" />
        <Button variant="ghost" size="sm" className="h-6 px-2 text-xs" onClick={onBack}>
          {t("wiki.back")}
        </Button>
      </div>
      {loading ? (
        <div className="flex justify-center py-10">
          <Loader2 size={20} className="animate-spin text-muted-foreground" />
        </div>
      ) : hits.length === 0 ? (
        <p className="py-10 text-center text-sm text-muted-foreground">{t("wiki.noResults")}</p>
      ) : (
        <div className="flex flex-col">
          {hits.map((h) => (
            <button
              key={h.uid}
              className="flex flex-col gap-0.5 border-b px-4 py-2.5 text-left hover:bg-accent/50"
              onClick={() => onOpen(h.uid)}
            >
              <div className="flex items-center gap-2">
                <LayerBadge layer={h.layer} />
                <span className="truncate text-sm font-medium">{h.title}</span>
                <span className="ml-auto shrink-0 text-xs text-muted-foreground">
                  {t("wiki.score")} {h.score.toFixed(1)}
                </span>
              </div>
              {h.node_path && (
                <span className="text-xs text-muted-foreground">{h.node_path}</span>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function LayerBadge({ layer }: { layer: string }) {
  const { t } = useTranslation();
  // i18n key 用复数目录名（wiki.sections.pages/lists/reports/entities）
  const sectionKey =
    layer === "Page"
      ? "pages"
      : layer === "List"
        ? "lists"
        : layer === "Report"
          ? "reports"
          : "entities";
  const cls =
    layer === "Page"
      ? "border-blue-500/30 bg-blue-500/10 text-blue-600 dark:text-blue-400"
      : layer === "List"
        ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
        : layer === "Report"
          ? "border-amber-500/30 bg-amber-500/10 text-amber-600 dark:text-amber-400"
          : "border-purple-500/30 bg-purple-500/10 text-purple-600 dark:text-purple-400";
  return (
    <span className={`shrink-0 rounded border px-1 py-px text-[10px] font-medium ${cls}`}>
      {t(`wiki.sections.${sectionKey}`)}
    </span>
  );
}
