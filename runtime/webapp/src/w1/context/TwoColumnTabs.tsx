//! 【第二层:全域双栏联动交互区装配】(#22 拆分:自 context.tsx 机械移入)
//! Tab 栏(人设/工具/记忆/文件/暴增/轨迹)+ 全局 Raw 报文开关;
//! 六个视图块各自成文件(tabs/),选中态与复制反馈经 props 上下贯通。

import {
  Layers,
  Wrench,
  MessageSquare,
  Code2,
  Activity,
  FileCode,
  TrendingUp,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { CtxStep } from "@/w2/api";
import type { ParsedPromptRecipe } from "./utils";
import type { ContextStats, SpikeItem } from "./types";
import { RecipeTab } from "./tabs/RecipeTab";
import { ToolsTab } from "./tabs/ToolsTab";
import { MemoryTab } from "./tabs/MemoryTab";
import { FilesTab } from "./tabs/FilesTab";
import { SpikesTab } from "./tabs/SpikesTab";
import { TrajectoryTab } from "./tabs/TrajectoryTab";

export type ContextTab = "recipe" | "tools" | "memory" | "files" | "spikes" | "trajectory";

export function TwoColumnTabs({
  activeTab,
  onTabChange,
  showRawJson,
  onToggleRawJson,
  recipe,
  latestSnapshot,
  stats,
  visible,
  selectedPromptSection,
  onSelectSection,
  selectedToolName,
  onSelectTool,
  selectedTurnIndex,
  onSelectTurn,
  selectedFileIndex,
  onSelectFile,
  copiedKey,
  onCopy,
  spikeItems,
}: {
  activeTab: ContextTab;
  onTabChange: (t: ContextTab) => void;
  showRawJson: boolean;
  onToggleRawJson: () => void;
  recipe: ParsedPromptRecipe;
  latestSnapshot: CtxStep | null;
  stats: ContextStats | null;
  visible: CtxStep[];
  selectedPromptSection: string;
  onSelectSection: (id: string) => void;
  selectedToolName: string | null;
  onSelectTool: (name: string) => void;
  selectedTurnIndex: number | null;
  onSelectTurn: (idx: number) => void;
  selectedFileIndex: number | null;
  onSelectFile: (idx: number) => void;
  copiedKey: string | null;
  onCopy: (key: string, text: string) => void;
  spikeItems: SpikeItem[];
}) {
  const tabCls = (active: boolean) =>
    cn(
      "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12.5px] font-medium transition-colors",
      active
        ? "bg-primary text-primary-foreground"
        : "text-muted-foreground hover:bg-muted",
    );
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 relative z-10">
      <div className="flex items-center justify-between border-b pb-1">
        <div className="flex flex-wrap items-center gap-1" role="tablist">
          <button role="tab" onClick={() => onTabChange("recipe")} className={tabCls(activeTab === "recipe")}>
            <Layers className="size-3.5" />
            <span>人设与特长双栏</span>
          </button>

          <button role="tab" onClick={() => onTabChange("tools")} className={tabCls(activeTab === "tools")}>
            <Wrench className="size-3.5" />
            <span>工具背包双栏 ({recipe.toolList.length})</span>
          </button>

          <button role="tab" onClick={() => onTabChange("memory")} className={tabCls(activeTab === "memory")}>
            <MessageSquare className="size-3.5" />
            <span>聊天记忆双栏 ({recipe.historyTurns.length}轮)</span>
          </button>

          <button role="tab" onClick={() => onTabChange("files")} className={tabCls(activeTab === "files")}>
            <FileCode className="size-3.5" />
            <span>工程文件副作用 ({recipe.affectedFiles.length})</span>
          </button>

          <button role="tab" onClick={() => onTabChange("spikes")} className={tabCls(activeTab === "spikes")}>
            <TrendingUp className="size-3.5" />
            <span>Token暴增诊断</span>
          </button>

          <button role="tab" onClick={() => onTabChange("trajectory")} className={tabCls(activeTab === "trajectory")}>
            <Activity className="size-3.5" />
            <span>步骤时序流</span>
          </button>
        </div>

        <div className="flex items-center gap-1.5">
          <Button
            size="sm"
            variant={showRawJson ? "secondary" : "ghost"}
            className="h-7 gap-1 px-2 text-[11.5px] text-muted-foreground"
            onClick={onToggleRawJson}
            title="切换查看全部发给模型的原始 JSON 报文"
          >
            <Code2 className="size-3.5" />
            <span>{showRawJson ? "返回大白话" : "全局 Raw 报文"}</span>
          </Button>
        </div>
      </div>

      {/* 全局专家模式展示 Raw JSON */}
      {showRawJson ? (
        <div className="min-h-0 flex-1 overflow-auto rounded-xl border bg-muted/20 p-3">
          <div className="mb-2 flex items-center justify-between text-[12px] font-medium">
            <span>底层完整请求报文 (OpenAI API 格式)</span>
            <span className="text-muted-foreground font-mono">
              {latestSnapshot?.messages?.length ?? 0} messages · {latestSnapshot?.tools?.length ?? 0} tools
            </span>
          </div>
          <pre className="max-h-[500px] overflow-auto rounded-lg border bg-background/80 p-3 font-mono text-[11.5px] leading-relaxed break-all whitespace-pre-wrap">
            {JSON.stringify(
              {
                model: latestSnapshot?.model_id,
                messages: latestSnapshot?.messages,
                tools: latestSnapshot?.tools,
              },
              null,
              2,
            )}
          </pre>
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-auto">
          {activeTab === "recipe" ? (
            <RecipeTab
              recipe={recipe}
              stats={stats}
              selectedPromptSection={selectedPromptSection}
              onSelectSection={onSelectSection}
              copiedKey={copiedKey}
              onCopy={onCopy}
            />
          ) : null}

          {activeTab === "tools" ? (
            <ToolsTab
              recipe={recipe}
              stats={stats}
              selectedToolName={selectedToolName}
              onSelectTool={onSelectTool}
              copiedKey={copiedKey}
              onCopy={onCopy}
            />
          ) : null}

          {activeTab === "memory" ? (
            <MemoryTab
              recipe={recipe}
              stats={stats}
              selectedTurnIndex={selectedTurnIndex}
              onSelectTurn={onSelectTurn}
              copiedKey={copiedKey}
              onCopy={onCopy}
            />
          ) : null}

          {activeTab === "files" ? (
            <FilesTab
              recipe={recipe}
              selectedFileIndex={selectedFileIndex}
              onSelectFile={onSelectFile}
              onCopy={onCopy}
            />
          ) : null}

          {activeTab === "spikes" ? <SpikesTab items={spikeItems} /> : null}

          {activeTab === "trajectory" ? (
            <TrajectoryTab visible={visible} recipe={recipe} stats={stats} />
          ) : null}
        </div>
      )}
    </div>
  );
}
