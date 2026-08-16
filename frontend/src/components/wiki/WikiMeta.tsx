/**
 * WIKI 右栏：Tab 叠放【关系 / 对话】。
 * 关系 = 活动节点出边（LRU 顺序，添加/删除）；对话 = ChatPane panel 形态
 * scene="wiki"（一软件一会话，挂载即 ensureAppSession）。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { GitFork, Loader2, MessageSquare, Plus, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { api, type WikiNode, type WikiRelation } from "@/api/client";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { toast } from "sonner";
import { ChatPane } from "@/components/chat/ChatPane";
import { useAppStore } from "@/stores/app-store";

type TabKey = "relations" | "chat";

export function WikiMeta({
  activeUid,
  activeNode,
  onOpenNode,
  onChanged,
}: {
  activeUid: string | null;
  activeNode?: WikiNode;
  onOpenNode: (uid: string) => void;
  /** 关系变更后刷新（重读活动节点 + 树） */
  onChanged: (uid?: string) => void;
}) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<TabKey>("relations");
  const [rels, setRels] = useState<WikiRelation[]>([]);
  const [loading, setLoading] = useState(false);
  const [adding, setAdding] = useState(false);
  const [toUid, setToUid] = useState("");
  const [relName, setRelName] = useState("");
  const [comment, setComment] = useState("");

  // 活动节点变化 → 拉取关系。依赖 activeNode：编辑/关系写回后 WikiApp 重读
  // 节点产生新对象 → 这里重拉（关系可能已变）。
  useEffect(() => {
    setRels([]);
    if (!activeUid) return;
    setLoading(true);
    api
      .wikiRelations(activeUid)
      .then((res) => setRels(res.edges))
      .catch(() => setRels([]))
      .finally(() => setLoading(false));
  }, [activeUid, activeNode]);

  const addRelation = async () => {
    if (!activeUid) return;
    if (!toUid.trim() || !relName.trim()) {
      toast.error(t("wiki.relations.required"));
      return;
    }
    try {
      await api.wikiAddRelation({
        from_uid: activeUid,
        to_uid: toUid.trim(),
        relation_name: relName.trim(),
        comment: comment.trim() || undefined,
      });
      toast.success(t("wiki.relations.added"));
      setToUid("");
      setRelName("");
      setComment("");
      setAdding(false);
      await onChanged(activeUid);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  };

  const removeRelation = async (r: WikiRelation) => {
    if (!activeUid) return;
    try {
      await api.wikiRemoveRelation({
        from_uid: activeUid,
        to_uid: r.to_uid,
        relation_name: r.relation_name,
      });
      await onChanged(activeUid);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Tab 头 */}
      <div className="flex shrink-0 border-b">
        <TabButton active={tab === "relations"} onClick={() => setTab("relations")}>
          <GitFork size={12} />
          {t("wiki.relations.title")}
          {activeUid && <span className="text-[10px] text-muted-foreground">{rels.length}</span>}
        </TabButton>
        <TabButton active={tab === "chat"} onClick={() => setTab("chat")}>
          <MessageSquare size={12} />
          {t("wiki.chat")}
        </TabButton>
      </div>

      {tab === "relations" ? (
        <div className="min-h-0 flex-1 overflow-y-auto">
          {!activeUid ? (
            <p className="px-4 py-8 text-center text-xs text-muted-foreground">
              {t("wiki.relations.noNode")}
            </p>
          ) : loading ? (
            <div className="flex justify-center py-8">
              <Loader2 size={16} className="animate-spin text-muted-foreground" />
            </div>
          ) : (
            <div className="flex flex-col">
              {/* 添加表单 */}
              {adding ? (
                <div className="flex flex-col gap-1.5 border-b bg-muted/20 p-2.5">
                  <Input
                    value={toUid}
                    onChange={(e) => setToUid(e.target.value)}
                    className="h-7 text-xs"
                    placeholder={t("wiki.relations.toUid")}
                  />
                  <Input
                    value={relName}
                    onChange={(e) => setRelName(e.target.value)}
                    className="h-7 text-xs"
                    placeholder={t("wiki.relations.name")}
                  />
                  <Input
                    value={comment}
                    onChange={(e) => setComment(e.target.value)}
                    className="h-7 text-xs"
                    placeholder={t("wiki.relations.comment")}
                  />
                  <div className="flex justify-end gap-1.5">
                    <Button variant="ghost" size="sm" className="h-6 px-2 text-[11px]" onClick={() => setAdding(false)}>
                      {t("wiki.editor.cancel")}
                    </Button>
                    <Button size="sm" className="h-6 px-2.5 text-[11px]" onClick={() => void addRelation()}>
                      <Plus size={11} className="mr-1" />
                      {t("wiki.relations.add")}
                    </Button>
                  </div>
                </div>
              ) : (
                <button
                  className="flex items-center gap-1 border-b px-3 py-2 text-xs text-muted-foreground hover:bg-accent/40"
                  onClick={() => setAdding(true)}
                >
                  <Plus size={12} />
                  {t("wiki.relations.add")}
                </button>
              )}

              <p className="px-3 py-1.5 text-[10px] text-muted-foreground/70">
                {t("wiki.relationCap")}
              </p>

              {rels.length === 0 ? (
                <p className="px-4 py-6 text-center text-xs text-muted-foreground">
                  {t("wiki.relations.empty")}
                </p>
              ) : (
                rels.map((r) => (
                  <div key={`${r.to_uid}-${r.relation_name}`} className="group flex items-center gap-1.5 border-b px-3 py-1.5">
                    <span className="flex-1 truncate">
                      <button
                        className="font-mono text-[11px] text-primary hover:underline"
                        onClick={() => onOpenNode(r.to_uid)}
                        title={t("wiki.relations.open")}
                      >
                        {r.to_uid}
                      </button>
                      <span className="ml-1.5 text-[11px] text-muted-foreground">{r.relation_name}</span>
                    </span>
                    <button
                      className="hidden text-muted-foreground hover:text-destructive group-hover:block"
                      onClick={() => void removeRelation(r)}
                      title={t("wiki.relations.remove")}
                    >
                      <X size={12} />
                    </button>
                  </div>
                ))
              )}
            </div>
          )}
        </div>
      ) : (
        <WikiChatPanel />
      )}
    </div>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      className={cn(
        "flex flex-1 items-center justify-center gap-1.5 border-b-2 px-2 py-2 text-xs font-medium",
        active
          ? "border-primary text-foreground"
          : "border-transparent text-muted-foreground hover:text-foreground",
      )}
      onClick={onClick}
    >
      {children}
    </button>
  );
}

/** 对话面板：panel 形态 + wiki 场景（一软件一会话） */
function WikiChatPanel() {
  const ensureAppSession = useAppStore((s) => s.ensureAppSession);
  useEffect(() => {
    void ensureAppSession("wiki");
  }, [ensureAppSession]);
  return (
    <div className="min-h-0 flex-1">
      <ChatPane variant="panel" scene="wiki" />
    </div>
  );
}
