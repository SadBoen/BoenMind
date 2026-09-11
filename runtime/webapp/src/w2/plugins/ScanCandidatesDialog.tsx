//! 插件扫描发现对话框(#22 拆分:自 PluginsPage.tsx 机械移入)。
//! 展示扫描路径与候选清单;批准动作经 onApprove 上抛(装配层驱动 API)。

import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import type { McpCandidatesResult } from "./types";

export function ScanCandidatesDialog({
  result,
  approving,
  onClose,
  onApprove,
}: {
  result: McpCandidatesResult;
  approving: string | null;
  onClose: () => void;
  onApprove: (candidate: McpCandidatesResult["candidates"][number]) => void;
}) {
  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>插件目录扫描</DialogTitle>
          <DialogDescription>
            扫描路径: {result.dir}
            {result.bundled_dir ? ` · 随包目录: ${result.bundled_dir}` : ""}
          </DialogDescription>
        </DialogHeader>
        <div className="bg-muted/60 text-muted-foreground rounded-md border px-3 py-2 text-xs leading-relaxed">
          扫描已以 <code className="font-mono">--self-describe</code> 参数运行下列候选可执行文件以读取其自报声明(识别插件身份的唯一方式)。文件在「批准接入」前不会写入配置或上线。
        </div>
        {result.candidates.length ? (
          <div className="max-h-72 space-y-2 overflow-auto">
            {result.candidates.map((c) => (
              <div
                key={c.name}
                className="flex items-start justify-between gap-3 rounded-lg border p-2.5"
              >
                <div className="min-w-0">
                  <div className="text-sm font-medium">
                    {c.title || c.name}{" "}
                    {c.source === "bundled" ? (
                      <span className="text-muted-foreground text-xs">(官方随包)</span>
                    ) : null}
                    {c.registered ? (
                      <span className="text-emerald-600 text-xs ml-1">(已登记)</span>
                    ) : null}
                    {c.tombstoned && !c.registered ? (
                      <span className="text-muted-foreground text-xs ml-1">
                        {c.source === "bundled"
                          ? "(未自动启用 · 可重新接入)"
                          : "(已移除 · 可恢复接入)"}
                      </span>
                    ) : null}
                  </div>
                  <div className="text-muted-foreground truncate text-xs mt-0.5">
                    {c.description || c.file}
                  </div>
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={c.registered || approving === c.name}
                  onClick={() => onApprove(c)}
                >
                  {c.registered
                    ? "已批准"
                    : c.tombstoned
                      ? c.source === "bundled"
                        ? "批准接入"
                        : "批准恢复"
                      : "批准接入"}
                </Button>
              </div>
            ))}
          </div>
        ) : (
          <div className="text-muted-foreground py-6 text-center text-[12.5px]">
            未在扫描路径中发现新的插件可执行文件。
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            关闭
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
