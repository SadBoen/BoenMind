//! 子进程 stderr 回看弹窗(issue #28;#22 拆分:自 PluginsPage.tsx 机械移入)。
//! 环形缓冲尾部 200 行,跨重启带代标记;刷新动作经 onRefresh 上抛。

import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Loader2Icon, RefreshCwIcon } from "lucide-react";

export type StderrViewState = {
  name: string;
  loading: boolean;
  lines: { generation: number; text: string }[];
  error?: string;
};

export function StderrDialog({
  view,
  onClose,
  onRefresh,
}: {
  view: StderrViewState;
  onClose: () => void;
  onRefresh: () => void;
}) {
  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="sm:max-w-2xl" data-slot="mcp-stderr-dialog">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            stderr · {view.name}
            <Button
              variant="ghost"
              size="sm"
              className="h-6 px-2 text-[11px]"
              disabled={view.loading}
              onClick={onRefresh}
            >
              <RefreshCwIcon className="size-3" />
              刷新
            </Button>
          </DialogTitle>
          <DialogDescription>
            子进程 stderr 尾部(最近 200 行,环形缓冲;「第 N 代」= 重启代数)
          </DialogDescription>
        </DialogHeader>
        {view.loading ? (
          <div className="flex items-center gap-2 py-6 justify-center text-muted-foreground text-sm">
            <Loader2Icon className="size-4 animate-spin" /> 读取中…
          </div>
        ) : view.error ? (
          <div className="rounded-md border border-destructive/30 bg-destructive/10 text-destructive text-[12.5px] p-3">
            {view.error}
          </div>
        ) : view.lines.length === 0 ? (
          <div className="py-6 text-center text-muted-foreground text-[12.5px]">
            暂无 stderr 输出(子进程安静 = 好事)
          </div>
        ) : (
          <pre
            className="max-h-80 overflow-auto rounded-md bg-muted/40 border p-3 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap break-all"
            data-slot="mcp-stderr-body"
          >
            {view.lines
              .map((l) => `[g${l.generation}] ${l.text}`)
              .join("\n")}
          </pre>
        )}
      </DialogContent>
    </Dialog>
  );
}
