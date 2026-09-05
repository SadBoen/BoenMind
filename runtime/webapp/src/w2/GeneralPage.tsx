// W8 常规设置(ADR-0018):运行环境探针(Python/Node.js)+ 工作区注册表。
// 工作目录列表固定五行高度、超出滚动(用户明示);增删改/检测走 /admin/workspaces。
import { useCallback, useEffect, useState } from "react";
import { useTimedNotice } from "@/lib/use-timed-notice";
import {
  FolderOpenIcon,
  PencilIcon,
  PlusIcon,
  RefreshCwIcon,
  Trash2Icon,
} from "lucide-react";
import { api, type RuntimeEnv, type WorkspaceEntry } from "./api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { BM_EVENTS, emit } from "../lib/bus";

function ToolCard({
  title,
  info,
}: {
  title: string;
  info: RuntimeEnv["python"] | undefined;
}) {
  if (!info) {
    return (
      <div className="bg-card flex items-center gap-3 rounded-xl border p-3">
        <span className="text-muted-foreground text-[13px]">检测中…</span>
      </div>
    );
  }
  return (
    <div className="bg-card flex items-center gap-3 rounded-xl border p-3" data-slot="runtime-tool" data-tool={title}>
      <span
        className={cn(
          "flex size-9 shrink-0 items-center justify-center rounded-full text-white",
          info.installed ? "bg-[var(--state-success-fg)]" : "bg-muted-foreground/40",
        )}
      >
        <FolderOpenIcon className="size-4" />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-[13px] font-medium">{title}</span>
          <Badge
            variant={info.installed ? "default" : "secondary"}
            className="h-5 px-1.5 text-[10.5px]"
          >
            {info.installed ? "已安装" : "未检测到"}
          </Badge>
        </div>
        <div className="text-muted-foreground truncate font-mono text-[11.5px]">
          {info.installed
            ? `${info.version ?? ""} · ${info.program ?? ""}`
            : (info.error ?? "未检测到")}
        </div>
      </div>
    </div>
  );
}

type Draft = { id: string | null; name: string; path: string };
const EMPTY_DRAFT: Draft = { id: null, name: "", path: "" };

export function GeneralPage() {
  const [env, setEnv] = useState<RuntimeEnv | null>(null);
  const [probing, setProbing] = useState(false);
  const [workspaces, setWorkspaces] = useState<WorkspaceEntry[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const { notice, flash: flashNotice } = useTimedNotice(4000);
  const [error, setError] = useState<string | null>(null);

  const flash = (msg: string) => {
    flashNotice(msg);
    setError(null);
  };

  const loadEnv = useCallback(async () => {
    setProbing(true);
    try {
      setEnv(await api.runtimeEnv());
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setProbing(false);
    }
  }, []);

  const loadWorkspaces = useCallback(async () => {
    try {
      setWorkspaces((await api.workspaces.list()).workspaces);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  }, []);

  useEffect(() => {
    void loadEnv();
    void loadWorkspaces();
  }, [loadEnv, loadWorkspaces]);

  const saveDraft = async () => {
    if (!draft) return;
    if (!draft.name.trim()) {
      setError("名称不能为空");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (draft.id) {
        await api.workspaces.update(draft.id, {
          name: draft.name,
          path: draft.path,
        });
        flash("已保存");
      } else {
        await api.workspaces.create({ name: draft.name, path: draft.path });
        flash("已添加工作目录");
      }
      setDraft(null);
      await loadWorkspaces();
      // 通知输入框工作区候选刷新
      emit(BM_EVENTS.workspacesChanged);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  const removeWorkspace = async (w: WorkspaceEntry) => {
    setBusy(true);
    setError(null);
    try {
      await api.workspaces.remove(w.id);
      flash(`已移除「${w.name}」`);
      await loadWorkspaces();
      emit(BM_EVENTS.workspacesChanged);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  const checkOne = async (w: WorkspaceEntry) => {
    setError(null);
    try {
      const r = await api.workspaces.check(w.id);
      if (r.ok) flash(`「${w.name}」目录可用`);
      else setError(`${w.name}:${r.error ?? "不可用"}`);
      await loadWorkspaces();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  };

  return (
    <div className="flex flex-col gap-5">
      <div>
        <h2 className="text-[15px] font-semibold">常规</h2>
        <p className="text-muted-foreground text-[12.5px]">
          运行环境检测与工作目录(项目)管理;对话输入框可按会话选择工作目录。
        </p>
      </div>

      {/* 运行环境 */}
      <section className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <span className="text-[13px] font-medium">运行环境</span>
          <Button
            size="sm"
            variant="outline"
            className="h-7 px-2 text-[12px]"
            disabled={probing}
            data-slot="env-recheck"
            onClick={() => void loadEnv()}
          >
            <RefreshCwIcon className={cn("size-3.5", probing && "animate-spin")} />
            重新检测
          </Button>
        </div>
        <div className="grid gap-2 sm:grid-cols-2">
          <ToolCard title="Python" info={env?.python} />
          <ToolCard title="Node.js" info={env?.node} />
        </div>
      </section>

      {/* 工作目录:固定五行高度,超出滚动(用户明示) */}
      <section className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <span className="text-[13px] font-medium">工作目录</span>
          <span className="text-muted-foreground text-[11.5px]">
            每个项目一个路径;聊天输入框可按会话选择
          </span>
          <span className="flex-1" />
          <Button
            size="sm"
            className="h-7 px-2 text-[12px]"
            disabled={busy}
            data-slot="workspace-add"
            onClick={() => setDraft({ ...EMPTY_DRAFT })}
          >
            <PlusIcon className="size-3.5" />
            添加
          </Button>
        </div>
        <div
          className="overflow-y-auto rounded-xl border"
          style={{ height: "calc(5 * 57px)" }}
          data-slot="workspace-list"
        >
          {workspaces.map((w) => (
            <div
              key={w.id}
              className="flex items-center gap-2 border-b px-3 py-2 last:border-b-0"
              data-slot="workspace-row"
              data-id={w.id}
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate text-[13px] font-medium">{w.name}</span>
                  {w.isDefault ? (
                    <Badge variant="secondary" className="h-5 px-1.5 text-[10.5px]">
                      默认
                    </Badge>
                  ) : null}
                  {!w.exists ? (
                    <Badge variant="outline" className="text-destructive h-5 px-1.5 text-[10.5px]">
                      目录缺失
                    </Badge>
                  ) : null}
                </div>
                <div className="text-muted-foreground truncate font-mono text-[11.5px]">
                  {w.path}
                </div>
              </div>
              <Button
                size="sm"
                variant="ghost"
                className="h-7 px-2 text-[12px]"
                title="检测目录可用性"
                data-slot="workspace-check"
                onClick={() => void checkOne(w)}
              >
                <RefreshCwIcon className="size-3.5" />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                className="h-7 px-2 text-[12px]"
                title="编辑"
                data-slot="workspace-edit"
                onClick={() => setDraft({ id: w.id, name: w.name, path: w.path })}
              >
                <PencilIcon className="size-3.5" />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                className="text-destructive hover:bg-destructive/10 h-7 px-2 text-[12px]"
                title={w.isDefault ? "默认工作区不可删除" : "删除"}
                disabled={w.isDefault || busy}
                data-slot="workspace-delete"
                onClick={() => void removeWorkspace(w)}
              >
                <Trash2Icon className="size-3.5" />
              </Button>
            </div>
          ))}
          {workspaces.length === 0 ? (
            <div className="text-muted-foreground flex items-center justify-center px-3 py-6 text-[12.5px]">
              暂无工作目录,点「添加」登记第一个项目
            </div>
          ) : null}
        </div>
      </section>

      {notice ? (
        <div className="notice-success" data-slot="workspace-notice">
          {notice}
        </div>
      ) : null}
      {error ? (
        <div className="notice-error" data-slot="workspace-error">
          {error}
        </div>
      ) : null}

      {/* W9:账号与安全(改登录密码) */}
      <AccountSecurity />

      {/* 新增/编辑对话框 */}
      <Dialog open={draft !== null} onOpenChange={(o) => !o && setDraft(null)}>
        <DialogContent data-slot="workspace-dialog">
          <DialogHeader>
            <DialogTitle>{draft?.id ? "编辑工作目录" : "添加工作目录"}</DialogTitle>
          </DialogHeader>
          <div className="flex flex-col gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="ws-name">名称</Label>
              <Input
                id="ws-name"
                value={draft?.name ?? ""}
                placeholder="如:BoenMind 项目"
                onChange={(e) =>
                  setDraft((d) => (d ? { ...d, name: e.target.value } : d))
                }
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="ws-path">路径</Label>
              <Input
                id="ws-path"
                value={draft?.path ?? ""}
                placeholder="本机绝对路径,如 D:\projects\demo"
                onChange={(e) =>
                  setDraft((d) => (d ? { ...d, path: e.target.value } : d))
                }
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDraft(null)}>
              取消
            </Button>
            <Button disabled={busy} data-slot="workspace-save" onClick={() => void saveDraft()}>
              保存
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}


// W9:账号与安全——修改网页登录密码(改后服务端作废全部会话,各端重登)
function AccountSecurity() {
  const [oldPw, setOldPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const save = async () => {
    if (newPw.length < 6) {
      setMsg({ ok: false, text: "新密码至少 6 位" });
      return;
    }
    setBusy(true);
    try {
      const r = await api.portal.changePassword(oldPw, newPw);
      setMsg({ ok: true, text: r.note ?? "密码已更新,请重新登录" });
      setOldPw("");
      setNewPw("");
    } catch (e) {
      setMsg({ ok: false, text: e instanceof Error ? e.message : "修改失败" });
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="flex flex-col gap-3" data-slot="account-security">
      <div>
        <h2 className="text-[15px] font-semibold">账号与安全</h2>
        <p className="text-muted-foreground text-[12.5px]">
          修改网页登录密码;修改后所有已登录设备需重新登录。
        </p>
      </div>
      <div className="bg-card flex flex-col gap-3 rounded-xl border p-3">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="pw-old">旧密码</Label>
          <Input
            id="pw-old"
            type="password"
            value={oldPw}
            autoComplete="current-password"
            onChange={(e) => setOldPw(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="pw-new">新密码(至少 6 位)</Label>
          <Input
            id="pw-new"
            type="password"
            value={newPw}
            autoComplete="new-password"
            onChange={(e) => setNewPw(e.target.value)}
          />
        </div>
        <Button
          size="sm"
          className="w-fit"
          disabled={busy || !oldPw || !newPw}
          data-slot="pw-save"
          onClick={() => void save()}
        >
          修改密码
        </Button>
        {msg ? (
          <div
            className={cn(msg.ok ? "notice-success" : "notice-error")}
            data-slot="pw-msg"
          >
            {msg.text}
          </div>
        ) : null}
      </div>
    </section>
  );
}
