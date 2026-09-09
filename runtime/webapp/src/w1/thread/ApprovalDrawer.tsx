// 抽屉式超薄悬浮审批条(#22 拆分:自 thread.tsx 机械移入):
// 与输入框同宽、高度接近单行、支持展开代码/批准/驳回/关闭。
import { useState } from "react";
import {
  ShieldAlert,
  ChevronUp,
  ChevronDown,
  Code2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useBoenmindApprovals, type ApprovalRequest } from "../runtime";

export function ApprovalDrawer() {
  const { pendingApprovals, respondApproval } = useBoenmindApprovals();
  if (pendingApprovals.length === 0) return null;
  return (
    <div className="mb-2 flex flex-col gap-2" data-slot="approval-cards">
      {pendingApprovals.map((a) => (
        <ApprovalDrawerItem
          key={a.approval_id}
          req={a}
          onRespond={respondApproval}
        />
      ))}
    </div>
  );
}

function ApprovalDrawerItem({
  req,
  onRespond,
}: {
  req: ApprovalRequest;
  onRespond: (id: string, d: "approve" | "deny") => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [dismissed, setDismissed] = useState(false);

  if (dismissed) return null;

  const argsText =
    req.args == null
      ? "(无调用参数)"
      : JSON.stringify(req.args, null, 2);

  const handleAction = async (decision: "approve" | "deny") => {
    if (busy) return;
    setBusy(true);
    try {
      await onRespond(req.approval_id, decision);
    } finally {
      setBusy(false);
    }
  };

  // 提炼简明高亮动作摘要
  const summary = (() => {
    const a = req.args as Record<string, unknown> | null;
    if (req.capability === "system.exec" && a?.command) {
      return {
        label: "执行命令",
        detail: typeof a.command === "string" ? a.command : JSON.stringify(a.command),
      };
    }
    if ((req.capability === "fs.write" || req.capability === "fs.edit") && a?.path) {
      return {
        label: req.capability === "fs.write" ? "写入文件" : "编辑文件",
        detail: String(a.path),
      };
    }
    if (req.capability === "fs.read" && a?.path) {
      return {
        label: "读取文件",
        detail: String(a.path),
      };
    }
    return {
      label: req.capability,
      detail: a ? JSON.stringify(a) : "请求执行",
    };
  })();

  // 审计修复(2026-09-08):exec 显式 cwd 在折叠态也要可见——审批时用户往往
  // 只看命令文本, cwd 是同等重要的执行边界信息(后端越界会拒绝)。
  const execCwd = (() => {
    if (req.capability !== "system.exec" || req.args == null) return null;
    const v = (req.args as Record<string, unknown>).cwd;
    return typeof v === "string" && v.trim() ? v : null;
  })();

  return (
    <div
      className="animate-in fade-in slide-in-from-bottom-2 duration-200 group relative w-full overflow-hidden rounded-xl border border-border/80 bg-card/95 shadow-md backdrop-blur-md transition-all hover:border-border"
      data-slot="approval-card"
      data-approval-id={req.approval_id}
      data-status={req.status}
    >
      {/* 单行超薄主条：高度与单行输入框/工具条相当 (约 38px) */}
      <div className="flex min-h-[38px] items-center justify-between gap-2 px-3 py-1.5">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <div className="flex size-5 shrink-0 items-center justify-center rounded-md bg-[var(--state-warn-bg)] text-[var(--state-warn-fg)]">
            <ShieldAlert className="size-3.5" />
          </div>
          <span className="shrink-0 text-[12px] font-semibold text-foreground">
            {summary.label}
          </span>
          <span
            className="truncate font-mono text-[11.5px] text-muted-foreground"
            title={summary.detail}
          >
            {summary.detail}
          </span>
          {execCwd && (
            <span
              className="flex min-w-0 max-w-[45%] shrink-0 items-center gap-1 rounded-md bg-destructive/10 px-1.5 py-0.5 font-mono text-[11px] text-destructive"
              title={`工作目录:${execCwd}(超出工作区白名单会被拒绝)`}
            >
              <span className="shrink-0 opacity-80">cwd:</span>
              <span className="truncate">{execCwd}</span>
            </span>
          )}
        </div>

        {/* 右侧紧凑操作按钮组 */}
        <div className="flex shrink-0 items-center gap-1.5">
          <Button
            size="sm"
            variant="ghost"
            className="h-7 gap-1 px-2 text-[11.5px] text-muted-foreground hover:text-foreground"
            data-slot="approval-expand"
            onClick={() => setExpanded(!expanded)}
            title={expanded ? "收起详细参数" : "展开查看详细代码/参数"}
          >
            <Code2 className="size-3.5" />
            <span>{expanded ? "收起" : "展开"}</span>
            {expanded ? (
              <ChevronUp className="size-3" />
            ) : (
              <ChevronDown className="size-3" />
            )}
          </Button>

          <Button
            size="sm"
            disabled={busy}
            className="h-7 px-2.5 text-[11.5px] font-medium shadow-xs"
            data-slot="approval-approve"
            onClick={() => void handleAction("approve")}
            title="允许本次工具调用执行"
          >
            ✓ 批准
          </Button>

          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            className="h-7 px-2 text-[11.5px] text-destructive hover:bg-destructive/10 hover:text-destructive"
            data-slot="approval-deny"
            onClick={() => void handleAction("deny")}
            title="驳回本次工具调用"
          >
            驳回
          </Button>

          <Button
            size="sm"
            variant="ghost"
            className="size-7 p-0 text-muted-foreground hover:text-foreground"
            data-slot="approval-close"
            onClick={() => setDismissed(true)}
            title="关闭悬浮条"
          >
            <X className="size-3.5" />
          </Button>
        </div>
      </div>

      {/* 展开区域：显示完整调用参数与代码详情 */}
      {expanded ? (
        <div className="border-t border-border/50 bg-muted/30 px-3 py-2">
          <div className="mb-1 flex items-center justify-between text-[10.5px] font-medium text-muted-foreground uppercase tracking-wider">
            <span>调用参数与代码详情 (Payload)</span>
            <span className="font-mono text-[10px]">{req.capability}</span>
          </div>
          <pre className="max-h-48 overflow-auto rounded-lg border border-border/60 bg-background/80 p-2.5 font-mono text-[11px] leading-relaxed text-foreground selection:bg-primary/20">
            {argsText}
          </pre>
        </div>
      ) : null}
    </div>
  );
}
