//! TAB 2: 工具背包双栏联动(#22 拆分:自 context.tsx 视图块机械移入)
//! 左侧工具大白话卡片,右侧 OpenAI Function JSON 定义滚动定位高亮。

import { Code2, Copy, Check, ShieldAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { scrollToId, type ParsedPromptRecipe } from "../utils";
import type { ContextStats } from "../types";

export function ToolsTab({
  recipe,
  stats,
  selectedToolName,
  onSelectTool,
  copiedKey,
  onCopy,
}: {
  recipe: ParsedPromptRecipe;
  stats: ContextStats | null;
  selectedToolName: string | null;
  onSelectTool: (name: string) => void;
  copiedKey: string | null;
  onCopy: (key: string, text: string) => void;
}) {
  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex flex-wrap items-center justify-between border-b pb-2 text-[12.5px]">
        <div>
          <span className="font-semibold text-foreground">🛠️ 随身装备的工具箱 (双栏联动透视)</span>
          <span className="ml-2 text-[11.5px] text-muted-foreground">
            点击左侧卡片，右侧专家代码自动滚动并高亮定位
          </span>
        </div>
        <div className="text-[12px] text-muted-foreground">
          装备 <strong className="text-foreground">{recipe.toolList.length}</strong> 个工具 · 占用约{" "}
          <strong className="text-foreground">{stats?.toolsTokens ?? 0} token</strong> ({stats?.pct.tools ?? 0}%)
        </div>
      </div>

      <div className="grid grid-cols-1 gap-3.5 lg:grid-cols-12 min-h-[440px]">
        {/* 左侧：工具大白话卡片列表 */}
        <div className="flex flex-col gap-2 overflow-y-auto pr-1 lg:col-span-5 max-h-[500px]">
          {recipe.toolList.map((t) => {
            const isSelected = selectedToolName === t.name;
            return (
              <div
                key={t.name}
                onClick={() => {
                  onSelectTool(t.name);
                  scrollToId(`tool-block-${t.name}`);
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
                    <span className={cn("size-2 rounded-full shrink-0", isSelected ? "bg-primary" : "bg-muted-foreground/50")} />
                    <span className="font-mono text-[13px] font-semibold text-foreground truncate">
                      {t.name}
                    </span>
                  </div>
                  {/* ADR-0055:工具表快照不含审批事实,不再由描述文案反推徽标(该
                      反推曾对生产描述失真);审批事实见对话内审批卡与 /admin 能力面 */}
                </div>

                <div className="text-[11.5px] text-muted-foreground leading-snug line-clamp-2">
                  {t.description || "无详细描述"}
                </div>

                <div className="flex items-center justify-between border-t border-border/40 pt-1.5 text-[11px] text-muted-foreground">
                  <span>定义消耗: 约 {t.paramTokens} token</span>
                  <span className={cn("text-[11px] font-medium", isSelected ? "text-primary" : "text-muted-foreground/60")}>
                    {isSelected ? "✓ 正在右侧查看代码" : "点击查看代码"}
                  </span>
                </div>
              </div>
            );
          })}
        </div>

        {/* 右侧：专家模式代码展示与定位加深 */}
        <div className="flex flex-col gap-2.5 overflow-y-auto rounded-xl border bg-muted/20 p-3 lg:col-span-7 max-h-[500px]">
          <div className="flex items-center justify-between border-b border-border/60 pb-1.5 text-[12px]">
            <span className="font-semibold text-foreground flex items-center gap-1.5">
              <Code2 className="size-3.5 text-primary" />
              <span>专家模式：OpenAI Function JSON 定义</span>
            </span>
            <span className="text-[11px] font-mono text-muted-foreground">
              当前选中: {selectedToolName || "全部"}
            </span>
          </div>

          <div className="flex flex-col gap-3">
            {recipe.toolList.map((t) => {
              const isSelected = selectedToolName === t.name;
              return (
                <div
                  key={t.name}
                  id={`tool-block-${t.name}`}
                  className={cn(
                    "rounded-lg border p-2.5 transition-all duration-200",
                    isSelected
                      ? "border-primary bg-primary/10 shadow-sm ring-1 ring-primary/30"
                      : "border-border/60 bg-background/70 hover:border-border",
                  )}
                >
                  <div className="mb-1.5 flex items-center justify-between text-[11.5px]">
                    <div className="flex items-center gap-1.5 font-mono font-medium">
                      <span className={cn("size-2 rounded-full", isSelected ? "bg-primary" : "bg-muted-foreground")} />
                      <span className="text-foreground">{t.name}</span>
                      <span className="text-muted-foreground text-[10.5px]">
                        (约 {t.paramTokens} token)
                      </span>
                    </div>
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-6 gap-1 px-1.5 text-[10.5px] text-muted-foreground hover:text-foreground"
                      onClick={() => onCopy(`tool_${t.name}`, JSON.stringify(t.rawSchema, null, 2))}
                      title="复制此工具的 JSON 定义"
                    >
                      {copiedKey === `tool_${t.name}` ? (
                        <>
                          <Check className="size-3 text-emerald-500" />
                          <span className="text-emerald-500">已复制</span>
                        </>
                      ) : (
                        <>
                          <Copy className="size-3" />
                          <span>复制代码</span>
                        </>
                      )}
                    </Button>
                  </div>

                  <pre className="max-h-48 overflow-auto rounded bg-muted/40 p-2 font-mono text-[11px] leading-relaxed text-foreground/90 whitespace-pre-wrap break-all">
                    {JSON.stringify(
                      {
                        name: t.name,
                        description: t.description,
                        parameters: t.rawSchema,
                      },
                      null,
                      2,
                    )}
                  </pre>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
