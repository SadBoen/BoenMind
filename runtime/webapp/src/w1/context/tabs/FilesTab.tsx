//! TAB 4: 本地工程文件读写副作用追踪(#22 拆分:自 context.tsx 机械移入)
//! 自动捕获本轮模型经 fs.read/fs.write/fs.edit 触碰的文件资产(对标 Pi-Web)。

import { FileCheck, FileEdit, Code2, Copy } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { scrollToId, type ParsedPromptRecipe } from "../utils";

export function FilesTab({
  recipe,
  selectedFileIndex,
  onSelectFile,
  onCopy,
}: {
  recipe: ParsedPromptRecipe;
  selectedFileIndex: number | null;
  onSelectFile: (idx: number) => void;
  onCopy: (key: string, text: string) => void;
}) {
  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex flex-wrap items-center justify-between border-b pb-2 text-[12.5px]">
        <div>
          <span className="font-semibold text-foreground">📁 本地工程文件读写副作用追踪 (对标 Pi-Web)</span>
          <span className="ml-2 text-[11.5px] text-muted-foreground">
            自动捕获本轮模型通过 fs.read / fs.write / fs.edit 触碰的文件资产
          </span>
        </div>
        <span className="text-[12px] text-muted-foreground">
          共捕获 <strong className="text-foreground">{recipe.affectedFiles.length}</strong> 个文件操作
        </span>
      </div>

      <div className="grid grid-cols-1 gap-3.5 lg:grid-cols-12 min-h-[440px]">
        {/* 左侧：文件操作清单 */}
        <div className="flex flex-col gap-2 overflow-y-auto pr-1 lg:col-span-5 max-h-[500px]">
          {recipe.affectedFiles.length === 0 ? (
            <div className="rounded-xl border bg-card p-6 text-center text-[12.5px] text-muted-foreground">
              本轮对话未触发本地文件读写操作 (纯问答交互)
            </div>
          ) : (
            recipe.affectedFiles.map((f, idx) => {
              const isSelected = selectedFileIndex === idx;
              return (
                <div
                  key={idx}
                  onClick={() => {
                    onSelectFile(idx);
                    scrollToId(`file-effect-${idx}`);
                  }}
                  className={cn(
                    "cursor-pointer rounded-lg border p-3 transition-all duration-150 flex flex-col justify-between gap-1.5",
                    isSelected
                      ? "border-primary bg-primary/10 shadow-xs ring-1 ring-primary/40"
                      : "bg-card hover:border-border hover:bg-muted/30 border-border/70",
                  )}
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="flex items-center gap-1.5 min-w-0">
                      {f.action === "write" || f.action === "edit" ? (
                        <FileEdit className="size-4 text-amber-500 shrink-0" />
                      ) : (
                        <FileCheck className="size-4 text-sky-500 shrink-0" />
                      )}
                      <span className="font-mono text-[12.5px] font-semibold text-foreground truncate">
                        {f.path}
                      </span>
                    </div>
                    <span className={cn(
                      "rounded px-1.5 py-0.5 text-[10.5px] font-medium shrink-0",
                      f.action === "write" ? "bg-amber-500/15 text-amber-600" : f.action === "edit" ? "bg-purple-500/15 text-purple-600" : "bg-sky-500/15 text-sky-600"
                    )}>
                      {f.action === "write" ? "写入" : f.action === "edit" ? "编辑修改" : "读取"}
                    </span>
                  </div>
                  <div className="text-[11px] text-muted-foreground">
                    触发工具: <span className="font-mono text-foreground">{f.toolName}</span>
                  </div>
                  <div className="flex items-center justify-between border-t border-border/40 pt-1.5 text-[11px]">
                    <span className="text-muted-foreground">文件副作用</span>
                    <span className={cn("font-medium", isSelected ? "text-primary" : "text-muted-foreground/60")}>
                      {isSelected ? "✓ 正在右侧高亮" : "点击查看参数明细"}
                    </span>
                  </div>
                </div>
              );
            })
          )}
        </div>

        {/* 右侧：操作参数与内容详情 */}
        <div className="flex flex-col gap-2.5 overflow-y-auto rounded-xl border bg-muted/20 p-3 lg:col-span-7 max-h-[500px]">
          <div className="flex items-center justify-between border-b border-border/60 pb-1.5 text-[12px]">
            <span className="font-semibold text-foreground flex items-center gap-1.5">
              <Code2 className="size-3.5 text-primary" />
              <span>文件操作指令与参数明细</span>
            </span>
          </div>

          <div className="flex flex-col gap-3">
            {recipe.affectedFiles.length === 0 ? (
              <div className="p-8 text-center text-[12px] text-muted-foreground">
                无文件变动明细
              </div>
            ) : (
              recipe.affectedFiles.map((f, idx) => {
                const isSelected = selectedFileIndex === idx;
                return (
                  <div
                    key={idx}
                    id={`file-effect-${idx}`}
                    className={cn(
                      "rounded-lg border p-2.5 transition-all duration-200",
                      isSelected
                        ? "border-primary bg-primary/10 shadow-sm ring-1 ring-primary/30"
                        : "border-border/60 bg-background/70 hover:border-border",
                    )}
                  >
                    <div className="mb-1.5 flex items-center justify-between text-[11.5px]">
                      <span className="font-mono font-semibold text-foreground">
                        {f.toolName} → {f.path}
                      </span>
                      <div className="flex items-center gap-1.5">
                        {/* 代码行数净值统计 (对标 DSH FileCard) */}
                        {f.linesAdded != null || f.linesRemoved != null ? (
                          <span className="flex items-center gap-1 font-mono text-[10.5px]">
                            {(f.linesAdded ?? 0) > 0 ? (
                              <span className="rounded bg-emerald-500/10 px-1.5 py-0.5 font-semibold text-emerald-600 dark:text-emerald-400">
                                +{f.linesAdded}
                              </span>
                            ) : null}
                            {(f.linesRemoved ?? 0) > 0 ? (
                              <span className="rounded bg-rose-500/10 px-1.5 py-0.5 font-semibold text-rose-600 dark:text-rose-400">
                                −{f.linesRemoved}
                              </span>
                            ) : null}
                            <span className="text-muted-foreground">行</span>
                          </span>
                        ) : null}
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-6 gap-1 px-1.5 text-[10.5px] text-muted-foreground hover:text-foreground"
                          onClick={() => onCopy(`file_${idx}`, f.detail)}
                        >
                          <Copy className="size-3" />
                          <span>复制明细</span>
                        </Button>
                      </div>
                    </div>
                    <pre className="max-h-48 overflow-auto rounded bg-muted/40 p-2 font-mono text-[11px] leading-relaxed text-foreground/90 whitespace-pre-wrap break-all">
                      {f.detail}
                    </pre>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
