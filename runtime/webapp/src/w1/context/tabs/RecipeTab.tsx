//! TAB 1: 人设与特长双栏联动(#22 拆分:自 context.tsx 视图块机械移入)
//! 左侧人设/技能/工作区卡片,点击右侧 System Prompt 原文段落平滑滚动高亮。

import { User, Sparkles, FolderOpen, Code2, Copy, Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { estTokens, scrollToId, type ParsedPromptRecipe } from "../utils";
import type { ContextStats } from "../types";

export function RecipeTab({
  recipe,
  stats,
  selectedPromptSection,
  onSelectSection,
  copiedKey,
  onCopy,
}: {
  recipe: ParsedPromptRecipe;
  stats: ContextStats | null;
  selectedPromptSection: string;
  onSelectSection: (id: string) => void;
  copiedKey: string | null;
  onCopy: (key: string, text: string) => void;
}) {
  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex flex-wrap items-center justify-between border-b pb-2 text-[12.5px]">
        <div>
          <span className="font-semibold text-foreground">🎭 人设与特长技能 (双栏联动透视)</span>
          <span className="ml-2 text-[11.5px] text-muted-foreground">
            点击左侧人设或特长，右侧系统提示词原文自动平滑滚动并加深高亮
          </span>
        </div>
        <div className="text-[12px] text-muted-foreground">
          合计消耗约 <strong className="text-foreground">{(stats?.personaTokens ?? 0) + (stats?.skillsTokens ?? 0) + (stats?.wsTokens ?? 0)} token</strong>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-3.5 lg:grid-cols-12 min-h-[440px]">
        {/* 左侧卡片列表 */}
        <div className="flex flex-col gap-2.5 overflow-y-auto pr-1 lg:col-span-5 max-h-[500px]">
          {/* 人设卡片 */}
          <div
            onClick={() => {
              onSelectSection("persona");
              scrollToId("prompt-section-persona");
            }}
            className={cn(
              "cursor-pointer rounded-lg border p-3 transition-all duration-150 flex flex-col justify-between gap-1.5",
              selectedPromptSection === "persona"
                ? "border-primary bg-primary/10 shadow-xs ring-1 ring-primary/40"
                : "bg-card hover:border-border hover:bg-muted/30 border-border/70",
            )}
          >
            <div className="flex items-start justify-between gap-2">
              <div className="flex items-center gap-1.5 min-w-0">
                <User className="size-4 text-indigo-500 shrink-0" />
                <span className="text-[13px] font-semibold text-foreground truncate">
                  🎭 AI 的人设与根本规矩
                </span>
              </div>
              <span className="rounded bg-muted px-1.5 py-0.5 text-[10.5px] text-muted-foreground shrink-0">
                约 {estTokens(recipe.personaText)} token
              </span>
            </div>
            <div className="text-[11.5px] text-muted-foreground leading-snug line-clamp-3">
              {recipe.personaText || "无特殊设定，默认以通用助手作答"}
            </div>
            <div className="flex items-center justify-between border-t border-border/40 pt-1.5 text-[11px]">
              <span className="text-muted-foreground">核心基底 Prompt</span>
              <span className={cn("font-medium", selectedPromptSection === "persona" ? "text-primary" : "text-muted-foreground/60")}>
                {selectedPromptSection === "persona" ? "✓ 正在右侧高亮" : "点击查看原文"}
              </span>
            </div>
          </div>

          {/* 附加特长列表 */}
          {recipe.skills.map((s) => {
            const isSelected = selectedPromptSection === s.id;
            return (
              <div
                key={s.id}
                onClick={() => {
                  onSelectSection(s.id);
                  scrollToId(`prompt-section-${s.id}`);
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
                    <Sparkles className="size-4 text-purple-500 shrink-0" />
                    <span className="text-[13px] font-semibold text-foreground truncate">
                      ⚡ 附加特长 · {s.name}
                    </span>
                  </div>
                  <span className="rounded bg-muted px-1.5 py-0.5 text-[10.5px] text-muted-foreground shrink-0">
                    约 {estTokens(s.instruction)} token
                  </span>
                </div>
                <div className="text-[11.5px] text-muted-foreground leading-snug line-clamp-3">
                  {s.instruction}
                </div>
                <div className="flex items-center justify-between border-t border-border/40 pt-1.5 text-[11px]">
                  <span className="text-muted-foreground">技能指令包</span>
                  <span className={cn("font-medium", isSelected ? "text-primary" : "text-muted-foreground/60")}>
                    {isSelected ? "✓ 正在右侧高亮" : "点击查看原文"}
                  </span>
                </div>
              </div>
            );
          })}

          {/* 工作目录卡片 */}
          {recipe.workspaceText ? (
            <div
              onClick={() => {
                onSelectSection("workspace");
                scrollToId("prompt-section-workspace");
              }}
              className={cn(
                "cursor-pointer rounded-lg border p-3 transition-all duration-150 flex flex-col justify-between gap-1.5",
                selectedPromptSection === "workspace"
                  ? "border-primary bg-primary/10 shadow-xs ring-1 ring-primary/40"
                  : "bg-card hover:border-border hover:bg-muted/30 border-border/70",
              )}
            >
              <div className="flex items-start justify-between gap-2">
                <div className="flex items-center gap-1.5 min-w-0">
                  <FolderOpen className="size-4 text-emerald-500 shrink-0" />
                  <span className="text-[13px] font-semibold text-foreground truncate">
                    📁 工作区环境路径
                  </span>
                </div>
                <span className="rounded bg-muted px-1.5 py-0.5 text-[10.5px] text-muted-foreground shrink-0">
                  约 {estTokens(recipe.workspaceText)} token
                </span>
              </div>
              <div className="text-[11.5px] text-muted-foreground leading-snug line-clamp-2">
                {recipe.workspaceText}
              </div>
              <div className="flex items-center justify-between border-t border-border/40 pt-1.5 text-[11px]">
                <span className="text-muted-foreground">环境注入约束</span>
                <span className={cn("font-medium", selectedPromptSection === "workspace" ? "text-primary" : "text-muted-foreground/60")}>
                  {selectedPromptSection === "workspace" ? "✓ 正在右侧高亮" : "点击查看原文"}
                </span>
              </div>
            </div>
          ) : null}
        </div>

        {/* 右侧：完整系统提示词原文段落展示与高亮 */}
        <div className="flex flex-col gap-2.5 overflow-y-auto rounded-xl border bg-muted/20 p-3 lg:col-span-7 max-h-[500px]">
          <div className="flex items-center justify-between border-b border-border/60 pb-1.5 text-[12px]">
            <span className="font-semibold text-foreground flex items-center gap-1.5">
              <Code2 className="size-3.5 text-primary" />
              <span>发给模型的 System Prompt 真实段落</span>
            </span>
            <Button
              size="sm"
              variant="ghost"
              className="h-6 gap-1 px-1.5 text-[10.5px] text-muted-foreground hover:text-foreground"
              onClick={() => onCopy("all_prompt", recipe.rawSystemPrompt)}
            >
              {copiedKey === "all_prompt" ? (
                <>
                  <Check className="size-3 text-emerald-500" />
                  <span className="text-emerald-500">已复制全文</span>
                </>
              ) : (
                <>
                  <Copy className="size-3" />
                  <span>复制提示词全文</span>
                </>
              )}
            </Button>
          </div>

          <div className="flex flex-col gap-3">
            <div
              id="prompt-section-persona"
              className={cn(
                "rounded-lg border p-2.5 transition-all duration-200",
                selectedPromptSection === "persona"
                  ? "border-primary bg-primary/10 shadow-sm ring-1 ring-primary/30"
                  : "border-border/60 bg-background/70 hover:border-border",
              )}
            >
              <div className="mb-1 flex items-center justify-between text-[11.5px] font-medium text-foreground">
                <span>【人设根本规矩】</span>
                <span className="text-muted-foreground text-[10.5px]">约 {estTokens(recipe.personaText)} token</span>
              </div>
              <pre className="max-h-40 overflow-auto rounded bg-muted/40 p-2 font-mono text-[11px] leading-relaxed text-foreground/90 whitespace-pre-wrap break-all">
                {recipe.personaText || "无特殊设定，默认以通用助手作答"}
              </pre>
            </div>

            {recipe.skills.map((s) => {
              const isSelected = selectedPromptSection === s.id;
              return (
                <div
                  key={s.id}
                  id={`prompt-section-${s.id}`}
                  className={cn(
                    "rounded-lg border p-2.5 transition-all duration-200",
                    isSelected
                      ? "border-primary bg-primary/10 shadow-sm ring-1 ring-primary/30"
                      : "border-border/60 bg-background/70 hover:border-border",
                  )}
                >
                  <div className="mb-1 flex items-center justify-between text-[11.5px] font-medium text-foreground">
                    <span>【附加技能 · {s.name}】</span>
                    <span className="text-muted-foreground text-[10.5px]">约 {estTokens(s.instruction)} token</span>
                  </div>
                  <pre className="max-h-40 overflow-auto rounded bg-muted/40 p-2 font-mono text-[11px] leading-relaxed text-foreground/90 whitespace-pre-wrap break-all">
                    {s.instruction}
                  </pre>
                </div>
              );
            })}

            {recipe.workspaceText ? (
              <div
                id="prompt-section-workspace"
                className={cn(
                  "rounded-lg border p-2.5 transition-all duration-200",
                  selectedPromptSection === "workspace"
                    ? "border-primary bg-primary/10 shadow-sm ring-1 ring-primary/30"
                    : "border-border/60 bg-background/70 hover:border-border",
                )}
              >
                <div className="mb-1 flex items-center justify-between text-[11.5px] font-medium text-foreground">
                  <span>【工作目录环境注入】</span>
                  <span className="text-muted-foreground text-[10.5px]">约 {estTokens(recipe.workspaceText)} token</span>
                </div>
                <pre className="max-h-24 overflow-auto rounded bg-muted/40 p-2 font-mono text-[11px] leading-relaxed text-foreground/90 whitespace-pre-wrap break-all">
                  {recipe.workspaceText}
                </pre>
              </div>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
