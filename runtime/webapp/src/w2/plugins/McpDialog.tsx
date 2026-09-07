//! MCP 信息弹窗(自 PluginsPage.tsx 机械移入)。
import { useState } from "react";
import { Loader2Icon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,

  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { type Draft } from "./types";

export function McpDialog({
  draft,
  busy,
  onClose,
  onSave,
}: {
  draft: Draft;
  busy: boolean;
  onClose: () => void;
  onSave: (d: Draft) => Promise<void>;
}) {
  const [form, setForm] = useState<Draft>(draft);
  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{form._editing ? "编辑插件" : "接入外部插件"}</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-3 py-2 text-[13px]">
          <div className="flex flex-col gap-1">
            <Label className="text-xs">插件名称</Label>
            <Input
              disabled={form._editing}
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="唯一标识，如 web-multisearch"
              className="h-8 font-mono text-xs"
            />
          </div>

          <div className="flex flex-col gap-1">
            <Label className="text-xs">通信协议 (Transport)</Label>
            <select
              value={form.transport}
              onChange={(e) =>
                setForm({
                  ...form,
                  transport: e.target.value as Draft["transport"],
                })
              }
              className="bo-select border-input bg-background h-8 rounded-lg border border-border bg-muted/40 px-2 font-mono text-[12px] text-foreground outline-none focus:ring-2 focus:ring-ring"
            >
              <option value="stdio">stdio (本地子进程)</option>
              <option value="sse">sse (远程流式 / Server-Sent Events)</option>
              <option value="http">http (远程 HTTP)</option>
              <option value="streamable-http">streamable-http (远程 Streamable HTTP)</option>
            </select>
          </div>

          {form.transport === "stdio" ? (
            <>
              <div className="flex flex-col gap-1">
                <Label className="text-xs">命令路径 (Command)</Label>
                <Input
                  value={form.command}
                  onChange={(e) => setForm({ ...form, command: e.target.value })}
                  placeholder="可执行文件完整路径"
                  className="h-8 font-mono text-xs"
                />
              </div>
              <div className="flex flex-col gap-1">
                <Label className="text-xs">命令行参数 (空格分隔)</Label>
                <Input
                  value={form.args}
                  onChange={(e) => setForm({ ...form, args: e.target.value })}
                  placeholder="如: --config /path/to/cfg"
                  className="h-8 font-mono text-xs"
                />
              </div>
            </>
          ) : (
            <>
              <div className="flex flex-col gap-1">
                <Label className="text-xs">远程服务 URL</Label>
                <Input
                  value={form.url}
                  onChange={(e) => setForm({ ...form, url: e.target.value })}
                  placeholder="https://example.com/mcp"
                  className="h-8 font-mono text-xs"
                />
              </div>
              <div className="flex flex-col gap-1">
                <Label className="text-xs">Bearer Token (可选)</Label>
                <Input
                  type="password"
                  value={form.bearer_token}
                  onChange={(e) => setForm({ ...form, bearer_token: e.target.value })}
                  className="h-8 font-mono text-xs"
                />
              </div>
            </>
          )}

          <div className="flex flex-col gap-1">
            <Label className="text-xs">环境变量 (KEY=VAL 每行一条)</Label>
            <textarea
              rows={2}
              value={form.env}
              onChange={(e) => setForm({ ...form, env: e.target.value })}
              className="border-input bg-background rounded-md border p-2 font-mono text-xs outline-none resize-none"
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button onClick={() => void onSave(form)} disabled={busy || !form.name.trim()}>
            {busy ? <Loader2Icon className="animate-spin" /> : null} 保存
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
