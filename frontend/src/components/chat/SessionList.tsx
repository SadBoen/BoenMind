/**
 * 会话列表：新建、搜索、切换、重命名、删除。
 */
import { useMemo, useState } from "react";
import { MoreHorizontal, Pencil, Plus, Search, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/stores/app-store";

function formatTime(ts: number): string {
  const d = new Date(ts * 1000);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  if (sameDay) return d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  return d.toLocaleDateString("zh-CN", { month: "numeric", day: "numeric" });
}

export function SessionList() {
  const sessions = useAppStore((s) => s.sessions);
  const activeSessionId = useAppStore((s) => s.activeSessionId);
  const selectSession = useAppStore((s) => s.selectSession);
  const createSession = useAppStore((s) => s.createSession);
  const renameSession = useAppStore((s) => s.renameSession);
  const removeSession = useAppStore((s) => s.removeSession);

  const [query, setQuery] = useState("");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) => s.title.toLowerCase().includes(q));
  }, [sessions, query]);

  const startRename = (id: string, title: string) => {
    setRenaming(id);
    setRenameValue(title);
  };

  const confirmRename = async () => {
    if (renaming && renameValue.trim()) {
      await renameSession(renaming, renameValue.trim());
    }
    setRenaming(null);
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 p-3 pb-2">
        <Button
          size="sm"
          className="flex-1 gap-1"
          onClick={() => void createSession()}
          title="新建对话"
        >
          <Plus size={14} />
          新建对话
        </Button>
      </div>
      <div className="px-3 pb-2">
        <div className="relative">
          <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索会话…"
            className="h-8 pl-8 text-xs"
          />
        </div>
      </div>

      <div className="flex-1 space-y-0.5 overflow-y-auto px-2 pb-2">
        {filtered.length === 0 && (
          <p className="px-2 py-6 text-center text-xs text-muted-foreground">
            {sessions.length === 0 ? "暂无对话，点击「新建对话」开始" : "没有匹配的会话"}
          </p>
        )}
        {filtered.map((session) => (
          <div
            key={session.id}
            role="button"
            tabIndex={0}
            onClick={() => void selectSession(session.id)}
            onKeyDown={(e) => e.key === "Enter" && void selectSession(session.id)}
            className={cn(
              "group flex cursor-pointer items-center gap-1 rounded-md px-2 py-1.5 text-sm",
              session.id === activeSessionId
                ? "bg-accent text-accent-foreground"
                : "hover:bg-accent/50",
            )}
          >
            {renaming === session.id ? (
              <Input
                autoFocus
                value={renameValue}
                onChange={(e) => setRenameValue(e.target.value)}
                onBlur={() => void confirmRename()}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void confirmRename();
                  if (e.key === "Escape") setRenaming(null);
                }}
                className="h-7 text-xs"
                onClick={(e) => e.stopPropagation()}
              />
            ) : (
              <>
                <div className="min-w-0 flex-1">
                  <p className="truncate font-medium">{session.title}</p>
                  <p className="text-[10px] text-muted-foreground">
                    {formatTime(session.updated_at)}
                  </p>
                </div>
                <DropdownMenu>
                  <DropdownMenuTrigger
                    className="flex h-6 w-6 items-center justify-center rounded opacity-0 transition-opacity group-hover:opacity-100 hover:bg-accent"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <MoreHorizontal size={14} />
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="w-32">
                    <DropdownMenuItem onClick={() => startRename(session.id, session.title)}>
                      <Pencil size={14} className="mr-2" />
                      重命名
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      variant="destructive"
                      onClick={() => void removeSession(session.id)}
                    >
                      <Trash2 size={14} className="mr-2" />
                      删除
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
