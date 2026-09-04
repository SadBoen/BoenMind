// context-inspector: 对话上下文透视与分析器
// 纯展示与诊断分析，不修改数据，不执行压缩
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  RefreshCw,
  Loader2,
  Search,
  User,
  Sparkles,
  FolderOpen,
  Wrench,
  MessageSquare,
  Scissors,
  HelpCircle,
  Code2,
  CheckCircle2,
  AlertCircle,
  Clock,
  ChevronDown,
  ChevronUp,
  FileText,
  Activity,
  Layers,
  ArrowRight,
  ShieldAlert,
} from "lucide-react";
import { api, type CtxStep } from "../w2/api";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { storage, STORAGE_KEYS } from "@/lib/storage";
import { cn } from "@/lib/utils";

// 估算中英文字数或 token (chars/3)
const estWords = (s?: string | null) => Math.max(0, s?.length ?? 0);
const estTokens = (s?: string | null) => Math.max(1, Math.ceil((s?.length ?? 0) / 3));

const fmtDur = (ms?: number | null) =>
  ms == null ? "—" : ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;

// 对 Prompt 的 system 内容进行结构拆解 (人设/技能/工作区)
interface ParsedPromptRecipe {
  personaText: string;
  skills: Array<{ name: string; instruction: string }>;
  workspaceText: string | null;
  historyTurns: Array<{ user: string; assistant: string }>;
  currentUserInput: string;
  toolList: Array<{
    name: string;
    description: string;
    needsApproval: boolean;
    paramTokens: number;
    rawSchema: any;
  }>;
}

function parseStepRecipe(step: CtxStep): ParsedPromptRecipe {
  let personaText = "";
  const skills: Array<{ name: string; instruction: string }> = [];
  let workspaceText: string | null = null;
  const historyTurns: Array<{ user: string; assistant: string }> = [];
  let currentUserInput = "";

  const messages = step.messages ?? [];

  // 1. 解析 System Prompt
  const sysMsg = messages.find((m) => m.role === "system");
  if (sysMsg && sysMsg.content) {
    let raw = sysMsg.content;

    // 提取工作区注入
    const wsIdx = raw.indexOf("[工作目录]");
    if (wsIdx !== -1) {
      workspaceText = raw.substring(wsIdx).trim();
      raw = raw.substring(0, wsIdx).trim();
    }

    // 提取技能包：[附加技能 · 技能名]
    const skillRegex = /\[附加技能 · ([^\]]+)\]\n([\s\S]*?)(?=\n\n\[附加技能|\n\n$|$)/g;
    let match: RegExpExecArray | null;
    let lastEnd = 0;
    const firstSkillIdx = raw.indexOf("[附加技能 · ");

    if (firstSkillIdx !== -1) {
      personaText = raw.substring(0, firstSkillIdx).trim();
      while ((match = skillRegex.exec(raw)) !== null) {
        skills.push({
          name: match[1].trim(),
          instruction: match[2].trim(),
        });
      }
    } else {
      personaText = raw.trim();
    }
  }

  // 2. 解析历史与当前提问
  // 除去开头的 system，后面的非 tool 消息中，最后一条 user 是当前提问，前面是历史
  const nonSys = messages.filter((m) => m.role === "user" || m.role === "assistant");
  if (nonSys.length > 0) {
    const last = nonSys[nonSys.length - 1];
    if (last.role === "user") {
      currentUserInput = last.content;
      // 其余的配对成历史轮次
      const prev = nonSys.slice(0, nonSys.length - 1);
      for (let i = 0; i < prev.length; i += 2) {
        const u = prev[i]?.role === "user" ? prev[i].content : "";
        const a = prev[i + 1]?.role === "assistant" ? prev[i + 1].content : "";
        if (u || a) {
          historyTurns.push({ user: u, assistant: a });
        }
      }
    }
  }

  // 3. 解析工具箱
  const toolList = (step.tools ?? []).map((t: any) => {
    const fn = t.function ?? {};
    const name = fn.name ?? "未知工具";
    const desc = fn.description ?? "";
    const needsApproval = desc.includes("需要用户审批");
    const paramStr = JSON.stringify(fn.parameters ?? {});
    return {
      name,
      description: desc,
      needsApproval,
      paramTokens: estTokens(paramStr),
      rawSchema: fn.parameters,
    };
  });

  return {
    personaText: personaText || "默认通用助理",
    skills,
    workspaceText,
    historyTurns,
    currentUserInput,
    toolList,
  };
}

export function ContextView() {
  const [steps, setSteps] = useState<CtxStep[]>([]);
  const [searchQ, setSearchQ] = useState("");
  const [searchHits, setSearchHits] = useState<CtxStep[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [auto, setAuto] = useState(true);
  const [onlyCurrent, setOnlyCurrent] = useState(true);

  // 展开状态控制
  const [openSeq, setOpenSeq] = useState<number | null>(null);
  const [activeTab, setActiveTab] = useState<"recipe" | "tools" | "memory" | "trajectory" | "raw">("recipe");
  const [showRawJson, setShowRawJson] = useState(false);

  const refresh = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.context();
      setSteps(r.steps);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!auto) return;
    const t = setInterval(() => void refresh(), 8_000);
    return () => clearInterval(t);
  }, [auto, refresh]);

  const sid = storage.get(STORAGE_KEYS.SESSION);

  // 过滤出当前会话并按时间由新到旧
  const visible = useMemo(() => {
    const list = onlyCurrent && sid ? steps.filter((s) => s.session_id === sid) : steps;
    return [...list].reverse();
  }, [steps, onlyCurrent, sid]);

  // 最近一次模型调用的快照 (无 kind 的行才是模型请求快照)
  const latestSnapshot = useMemo(() => {
    return visible.find((x) => !x.kind);
  }, [visible]);

  // 解析配方
  const recipe = useMemo(() => {
    if (!latestSnapshot) return null;
    return parseStepRecipe(latestSnapshot);
  }, [latestSnapshot]);

  // 篇幅与百分比计算
  const stats = useMemo(() => {
    if (!recipe || !latestSnapshot) return null;
    const personaTokens = estTokens(recipe.personaText);
    const skillsTokens = recipe.skills.reduce((sum, s) => sum + estTokens(s.instruction), 0);
    const wsTokens = recipe.workspaceText ? estTokens(recipe.workspaceText) : 0;
    const toolsTokens = recipe.toolList.reduce((sum, t) => sum + t.paramTokens, 0);
    const historyTokens = recipe.historyTurns.reduce(
      (sum, h) => sum + estTokens(h.user) + estTokens(h.assistant),
      0,
    );
    const inputTokens = estTokens(recipe.currentUserInput);

    const totalEst = personaTokens + skillsTokens + wsTokens + toolsTokens + historyTokens + inputTokens;
    const realTokensIn = latestSnapshot.tokens_in ?? totalEst;

    return {
      personaTokens,
      skillsTokens,
      wsTokens,
      toolsTokens,
      historyTokens,
      inputTokens,
      totalEst,
      realTokensIn,
      pct: {
        persona: Math.round((personaTokens / (totalEst || 1)) * 100),
        skills: Math.round((skillsTokens / (totalEst || 1)) * 100),
        ws: Math.round((wsTokens / (totalEst || 1)) * 100),
        tools: Math.round((toolsTokens / (totalEst || 1)) * 100),
        history: Math.round((historyTokens / (totalEst || 1)) * 100),
        input: Math.round((inputTokens / (totalEst || 1)) * 100),
      },
    };
  }, [recipe, latestSnapshot]);

  const runSearch = async () => {
    const q = searchQ.trim();
    if (!q) return;
    setSearching(true);
    try {
      const r = await api.contextSearch(q);
      setSearchHits(r.hits ?? []);
    } catch {
      setSearchHits([]);
    } finally {
      setSearching(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3.5 px-4 pb-4">
      {/* 顶部控制栏 */}
      <div className="flex flex-wrap items-center justify-between gap-2 border-b pb-2.5">
        <div className="flex items-center gap-2">
          <Activity className="size-4 text-primary" />
          <span className="text-[13px] font-semibold text-foreground">大模型交互透视分析</span>
          <span className="rounded-md bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground">
            只读透视 · 零压缩
          </span>
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <div className="flex items-center gap-1.5">
            <Switch
              id="ctx-only"
              checked={onlyCurrent && !!sid}
              onCheckedChange={setOnlyCurrent}
              disabled={!sid}
            />
            <Label htmlFor="ctx-only" className="cursor-pointer text-[12px] text-muted-foreground">
              仅看本会话
            </Label>
          </div>
          <div className="flex items-center gap-1.5">
            <Switch id="ctx-auto" checked={auto} onCheckedChange={setAuto} />
            <Label htmlFor="ctx-auto" className="cursor-pointer text-[12px] text-muted-foreground">
              实时自动刷新
            </Label>
          </div>
          <Button
            size="sm"
            variant="outline"
            className="h-7 gap-1 px-2.5 text-[12px]"
            disabled={busy}
            onClick={() => void refresh()}
          >
            {busy ? <Loader2 className="size-3.5 animate-spin" /> : <RefreshCw className="size-3.5" />}
            <span>刷新</span>
          </Button>
        </div>
      </div>

      {error ? <div className="notice-error">{error}</div> : null}

      {/* 【第一层：健康度看板 / 容量水杯】 */}
      {latestSnapshot && stats ? (
        <div className="bg-card rounded-xl border p-3.5 shadow-2xs">
          <div className="mb-2.5 flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <span className="text-[13px] font-semibold text-foreground">
                当前对话容量构成
              </span>
              <span className="flex items-center gap-1 rounded-full bg-emerald-500/10 px-2 py-0.5 text-[11px] font-medium text-emerald-600 dark:text-emerald-400">
                <CheckCircle2 className="size-3" />
                <span>记忆完整无遗漏 ({recipe?.historyTurns.length ?? 0}/20 轮)</span>
              </span>
            </div>

            <div className="flex items-center gap-3 text-[12px] text-muted-foreground">
              <span className="flex items-center gap-1">
                <Clock className="size-3.5" />
                思考耗时: <strong className="text-foreground">{fmtDur(latestSnapshot.latency_ms)}</strong>
              </span>
              <span>·</span>
              <span>
                本次提问篇幅: <strong className="text-foreground">{stats.realTokensIn}</strong> 字量 (Token)
              </span>
              <span>·</span>
              <span>模型: <strong className="text-foreground">{latestSnapshot.model_id}</strong></span>
            </div>
          </div>

          {/* 进度条水杯 */}
          <div className="flex h-3 w-full overflow-hidden rounded-full bg-muted/80">
            {stats.pct.persona > 0 ? (
              <div
                style={{ width: `${stats.pct.persona}%` }}
                className="bg-indigo-500 transition-all hover:opacity-80"
                title={`AI人设与规矩: 约 ${stats.personaTokens} 篇幅 (${stats.pct.persona}%)`}
              />
            ) : null}
            {stats.pct.skills > 0 ? (
              <div
                style={{ width: `${stats.pct.skills}%` }}
                className="bg-purple-500 transition-all hover:opacity-80"
                title={`携带特长技能: 约 ${stats.skillsTokens} 篇幅 (${stats.pct.skills}%)`}
              />
            ) : null}
            {stats.pct.tools > 0 ? (
              <div
                style={{ width: `${stats.pct.tools}%` }}
                className="bg-amber-500 transition-all hover:opacity-80"
                title={`装备工具箱: 约 ${stats.toolsTokens} 篇幅 (${stats.pct.tools}%)`}
              />
            ) : null}
            {stats.pct.history > 0 ? (
              <div
                style={{ width: `${stats.pct.history}%` }}
                className="bg-sky-500 transition-all hover:opacity-80"
                title={`之前聊天记忆: 约 ${stats.historyTokens} 篇幅 (${stats.pct.history}%)`}
              />
            ) : null}
            {stats.pct.ws > 0 ? (
              <div
                style={{ width: `${stats.pct.ws}%` }}
                className="bg-emerald-500 transition-all hover:opacity-80"
                title={`工作区环境: 约 ${stats.wsTokens} 篇幅 (${stats.pct.ws}%)`}
              />
            ) : null}
            {stats.pct.input > 0 ? (
              <div
                style={{ width: `${stats.pct.input}%` }}
                className="bg-rose-500 transition-all hover:opacity-80"
                title={`本次提问: 约 ${stats.inputTokens} 篇幅 (${stats.pct.input}%)`}
              />
            ) : null}
          </div>

          {/* 图例大白话对照表 */}
          <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1.5 text-[11.5px]">
            <span className="flex items-center gap-1.5">
              <span className="size-2.5 rounded-full bg-indigo-500" />
              <span className="text-foreground">🎭 人设规矩:</span>
              <span className="text-muted-foreground">{stats.pct.persona}%</span>
            </span>
            {stats.skillsTokens > 0 ? (
              <span className="flex items-center gap-1.5">
                <span className="size-2.5 rounded-full bg-purple-500" />
                <span className="text-foreground">⚡ 特长技能:</span>
                <span className="text-muted-foreground">{stats.pct.skills}%</span>
              </span>
            ) : null}
            <span className="flex items-center gap-1.5">
              <span className="size-2.5 rounded-full bg-amber-500" />
              <span className="text-foreground">🛠️ 工具背包:</span>
              <span className="text-muted-foreground">{stats.pct.tools}%</span>
            </span>
            <span className="flex items-center gap-1.5">
              <span className="size-2.5 rounded-full bg-sky-500" />
              <span className="text-foreground">💬 聊天记忆:</span>
              <span className="text-muted-foreground">{stats.pct.history}%</span>
            </span>
            {stats.wsTokens > 0 ? (
              <span className="flex items-center gap-1.5">
                <span className="size-2.5 rounded-full bg-emerald-500" />
                <span className="text-foreground">📁 电脑目录:</span>
                <span className="text-muted-foreground">{stats.pct.ws}%</span>
              </span>
            ) : null}
            <span className="flex items-center gap-1.5">
              <span className="size-2.5 rounded-full bg-rose-500" />
              <span className="text-foreground">❓ 您的问题:</span>
              <span className="text-muted-foreground">{stats.pct.input}%</span>
            </span>
          </div>
        </div>
      ) : (
        <div className="bg-card rounded-xl border p-6 text-center text-[12.5px] text-muted-foreground">
          {steps.length > 0
            ? "当前会话暂无调用记录，在左侧输入一句话发送后即可在此查看"
            : "尚未产生模型交互数据"}
        </div>
      )}

      {/* 【第二层：配方卡片盒 / 功能切页】 */}
      {recipe ? (
        <div className="flex min-h-0 flex-1 flex-col gap-3">
          <div className="flex items-center justify-between border-b pb-1">
            <div className="flex items-center gap-1" role="tablist">
              <button
                role="tab"
                onClick={() => setActiveTab("recipe")}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12.5px] font-medium transition-colors",
                  activeTab === "recipe"
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:bg-muted",
                )}
              >
                <Layers className="size-3.5" />
                <span>人设与技能 ({recipe.skills.length + 1})</span>
              </button>

              <button
                role="tab"
                onClick={() => setActiveTab("tools")}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12.5px] font-medium transition-colors",
                  activeTab === "tools"
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:bg-muted",
                )}
              >
                <Wrench className="size-3.5" />
                <span>工具背包 ({recipe.toolList.length})</span>
              </button>

              <button
                role="tab"
                onClick={() => setActiveTab("memory")}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12.5px] font-medium transition-colors",
                  activeTab === "memory"
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:bg-muted",
                )}
              >
                <MessageSquare className="size-3.5" />
                <span>聊天记忆 ({recipe.historyTurns.length}轮)</span>
              </button>

              <button
                role="tab"
                onClick={() => setActiveTab("trajectory")}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12.5px] font-medium transition-colors",
                  activeTab === "trajectory"
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:bg-muted",
                )}
              >
                <Activity className="size-3.5" />
                <span>步骤时序流</span>
              </button>
            </div>

            <div className="flex items-center gap-1.5">
              <Button
                size="sm"
                variant={showRawJson ? "secondary" : "ghost"}
                className="h-7 gap-1 px-2 text-[11.5px] text-muted-foreground"
                onClick={() => setShowRawJson(!showRawJson)}
                title="切换查看原始发给模型的 JSON 报文"
              >
                <Code2 className="size-3.5" />
                <span>{showRawJson ? "返回大白话" : "专家模式 (Raw)"}</span>
              </Button>
            </div>
          </div>

          {/* 专家模式直接展示 Raw JSON */}
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
              {/* TAB 1: 人设与技能 */}
              {activeTab === "recipe" ? (
                <div className="flex flex-col gap-3">
                  {/* 人设卡片 */}
                  <div className="bg-card rounded-xl border p-3.5 shadow-2xs">
                    <div className="mb-2 flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <User className="size-4 text-indigo-500" />
                        <span className="text-[13px] font-semibold">🎭 AI 的人设与根本规矩</span>
                      </div>
                      <span className="rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground">
                        篇幅: 约 {estTokens(recipe.personaText)}
                      </span>
                    </div>
                    <div className="rounded-lg bg-muted/40 p-3 text-[12.5px] leading-relaxed text-foreground/90 whitespace-pre-wrap">
                      {recipe.personaText || "无特殊规矩，默认以通用助手身份作答"}
                    </div>
                  </div>

                  {/* 附加特长技能 */}
                  {recipe.skills.length > 0 ? (
                    <div className="bg-card rounded-xl border p-3.5 shadow-2xs">
                      <div className="mb-2.5 flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          <Sparkles className="size-4 text-purple-500" />
                          <span className="text-[13px] font-semibold">⚡ 携带的特长技能知识包</span>
                        </div>
                        <span className="text-[11.5px] text-muted-foreground">
                          共携带 {recipe.skills.length} 项附加特长
                        </span>
                      </div>

                      <div className="flex flex-col gap-2.5">
                        {recipe.skills.map((s, idx) => (
                          <div key={idx} className="rounded-lg border bg-muted/20 p-2.5">
                            <div className="mb-1 flex items-center justify-between">
                              <span className="font-medium text-[12.5px] text-foreground">
                                【{s.name}】
                              </span>
                              <span className="text-[11px] text-muted-foreground">
                                约 {estTokens(s.instruction)} 篇幅
                              </span>
                            </div>
                            <div className="text-[12px] leading-relaxed text-muted-foreground whitespace-pre-wrap">
                              {s.instruction}
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}

                  {/* 工作目录注入 */}
                  {recipe.workspaceText ? (
                    <div className="bg-card rounded-xl border p-3.5 shadow-2xs">
                      <div className="mb-1.5 flex items-center gap-2">
                        <FolderOpen className="size-4 text-emerald-500" />
                        <span className="text-[13px] font-semibold">📁 允许查看与工作的电脑目录</span>
                      </div>
                      <div className="rounded-lg bg-muted/40 p-2.5 text-[12px] text-foreground/90">
                        {recipe.workspaceText}
                      </div>
                    </div>
                  ) : null}
                </div>
              ) : null}

              {/* TAB 2: 工具背包 */}
              {activeTab === "tools" ? (
                <div className="bg-card rounded-xl border p-3.5 shadow-2xs">
                  <div className="mb-3 flex items-center justify-between border-b pb-2">
                    <div>
                      <div className="text-[13px] font-semibold text-foreground">
                        🛠️ AI 随身装备的工具箱
                      </div>
                      <div className="text-[11.5px] text-muted-foreground">
                        这些是系统赋予 AI 的实际能力。工具的使用手册会占用背包空间，过多可能会让对话变慢。
                      </div>
                    </div>
                    <div className="text-right text-[12px]">
                      <div className="font-medium text-foreground">
                        共装备 {recipe.toolList.length} 个工具
                      </div>
                      <div className="text-[11px] text-muted-foreground">
                        背包占用 约 {stats?.toolsTokens ?? 0} 字量 ({stats?.pct.tools ?? 0}%)
                      </div>
                    </div>
                  </div>

                  <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                    {recipe.toolList.map((t, idx) => (
                      <div
                        key={idx}
                        className="flex flex-col justify-between rounded-lg border bg-muted/20 p-2.5 hover:bg-muted/40 transition-colors"
                      >
                        <div>
                          <div className="flex items-center justify-between gap-1.5 mb-1">
                            <span className="font-mono text-[12.5px] font-semibold text-foreground">
                              {t.name}
                            </span>
                            {t.needsApproval ? (
                              <span className="flex items-center gap-1 rounded bg-amber-500/10 px-1.5 py-0.5 text-[10.5px] font-medium text-amber-600 dark:text-amber-400">
                                <ShieldAlert className="size-3" />
                                <span>需人工确认</span>
                              </span>
                            ) : (
                              <span className="rounded bg-emerald-500/10 px-1.5 py-0.5 text-[10.5px] font-medium text-emerald-600 dark:text-emerald-400">
                                直通只读
                              </span>
                            )}
                          </div>
                          <div className="text-[11.5px] leading-snug text-muted-foreground">
                            {t.description || "无详细描述"}
                          </div>
                        </div>

                        <div className="mt-2.5 flex items-center justify-between border-t pt-1.5 text-[10.5px] text-muted-foreground">
                          <span>手册篇幅: 约 {t.paramTokens} 字</span>
                          <span className="font-mono">OpenAI Function</span>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}

              {/* TAB 3: 聊天记忆 */}
              {activeTab === "memory" ? (
                <div className="flex flex-col gap-3">
                  <div className="bg-card rounded-xl border p-3.5 shadow-2xs">
                    <div className="mb-2 flex items-center justify-between">
                      <div>
                        <div className="text-[13px] font-semibold text-foreground">
                          💬 AI 依然清晰保留的聊天记忆
                        </div>
                        <div className="text-[11.5px] text-muted-foreground">
                          系统自动保留最近的对话内容。如果对话超长，较早的对话将被自然移出。
                        </div>
                      </div>
                      <span className="rounded-md bg-sky-500/10 px-2 py-0.5 text-[11.5px] font-medium text-sky-600 dark:text-sky-400">
                        当前存活 {recipe.historyTurns.length} 轮 (上限 20 轮)
                      </span>
                    </div>

                    {recipe.historyTurns.length === 0 ? (
                      <div className="py-6 text-center text-[12.5px] text-muted-foreground">
                        这是新会话的第一轮对话，暂无前期记忆
                      </div>
                    ) : (
                      <div className="flex flex-col gap-2.5">
                        {recipe.historyTurns.map((h, idx) => (
                          <div key={idx} className="rounded-lg border bg-muted/20 p-2.5">
                            <div className="mb-1 flex items-center justify-between text-[11px] text-muted-foreground">
                              <span>第 {idx + 1} 轮记忆</span>
                              <span>
                                用户提问 约 {estTokens(h.user)} 字 · AI回答 约 {estTokens(h.assistant)} 字
                              </span>
                            </div>
                            <div className="mb-1 text-[12px] text-foreground/90 font-medium">
                              问: {h.user}
                            </div>
                            <div className="text-[11.5px] text-muted-foreground line-clamp-3">
                              答: {h.assistant}
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>

                  {/* 淘汰与裁剪状态提示卡片 */}
                  <div className="rounded-xl border border-dashed p-3 text-[12px] text-muted-foreground bg-muted/10 flex items-start gap-2.5">
                    <Scissors className="size-4 mt-0.5 text-muted-foreground shrink-0" />
                    <div>
                      <span className="font-semibold text-foreground">关于对话遗忘的说明：</span>
                      <span>
                        当前系统硬上限为 20 轮或 24,000 字符。目前您的会话长度健康，没有任何历史对话被剪掉。如果将来对话变长产生脱落，此处会明确提醒您遗忘了哪几轮，让您不再感到莫名其妙。
                      </span>
                    </div>
                  </div>
                </div>
              ) : null}

              {/* TAB 4: 步骤时序流 */}
              {activeTab === "trajectory" ? (
                <div className="flex flex-col gap-2 rounded-xl border bg-card p-3.5 shadow-2xs">
                  <div className="mb-2 text-[13px] font-semibold text-foreground">
                    ⚡ 最近一次互动的背后运行细节
                  </div>

                  {visible.length === 0 ? (
                    <div className="py-6 text-center text-[12.5px] text-muted-foreground">
                      暂无步骤事件
                    </div>
                  ) : (
                    visible.map((s) => {
                      if (s.kind) {
                        const d = (s.data ?? {}) as Record<string, unknown>;
                        const t = (() => {
                          const dd = new Date(s.ts);
                          return isNaN(dd.getTime()) ? s.ts : dd.toLocaleTimeString();
                        })();

                        const evMap: Record<string, { label: string; color: string; desc: string }> = {
                          tool_call: {
                            label: "AI 决定使用工具",
                            color: "text-amber-500 bg-amber-500/10 border-amber-500/20",
                            desc: `调用了 ${String(d.tool ?? "")}，参数为 ${JSON.stringify(d.arguments ?? {})}`,
                          },
                          tool_result: {
                            label: "工具完成并反馈",
                            color: "text-sky-500 bg-sky-500/10 border-sky-500/20",
                            desc: `耗时 ${fmtDur(d.elapsed_ms as number)}，返回结果已回喂给 AI`,
                          },
                          assistant_final: {
                            label: "AI 组织最终答复",
                            color: "text-purple-500 bg-purple-500/10 border-purple-500/20",
                            desc: `生成了答复，消耗输出约 ${String(d.tokens_out ?? "—")} 字`,
                          },
                          turn_end: {
                            label: "交互完满结束",
                            color: "text-emerald-500 bg-emerald-500/10 border-emerald-500/20",
                            desc: `本次对话顺利完成，总耗时 ${fmtDur(d.latency_ms as number)}`,
                          },
                        };

                        const meta = evMap[s.kind] ?? {
                          label: s.kind,
                          color: "text-muted-foreground bg-muted border-border",
                          desc: "",
                        };

                        return (
                          <div
                            key={s.seq}
                            className="flex items-start gap-3 rounded-lg border p-2.5 bg-muted/20 text-[12px]"
                          >
                            <span className={cn("rounded-md px-2 py-0.5 text-[11px] font-medium border shrink-0", meta.color)}>
                              {meta.label}
                            </span>
                            <div className="flex-1 min-w-0">
                              <div className="text-foreground font-medium">{meta.desc}</div>
                              {d.result || d.content ? (
                                <pre className="mt-1.5 max-h-32 overflow-auto rounded bg-background/80 p-2 font-mono text-[11px] text-muted-foreground whitespace-pre-wrap break-all">
                                  {String(d.result ?? d.content ?? "")}
                                </pre>
                              ) : null}
                            </div>
                            <span className="text-[11px] text-muted-foreground font-mono shrink-0">
                              {t}
                            </span>
                          </div>
                        );
                      }
                      return null;
                    })
                  )}
                </div>
              ) : null}
            </div>
          )}
        </div>
      ) : null}

      {/* 底部：跨会话搜索条 */}
      <div className="bg-card rounded-xl border p-3 shadow-2xs">
        <div className="mb-2 flex items-center justify-between text-[12.5px]">
          <span className="font-semibold text-foreground">🔍 跨会话查找曾发送的上下文或工具结果</span>
          <span className="text-[11.5px] text-muted-foreground">
            可以在历史所有问答中搜索某段代码或某次搜索结果
          </span>
        </div>
        <div className="flex gap-2">
          <input
            className="bg-background h-8 flex-1 rounded-md border px-2.5 text-[12px] outline-none focus:border-ring"
            placeholder="输入关键词搜索（如：天气 / fs__read / 某个报错）"
            value={searchQ}
            onChange={(e) => setSearchQ(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void runSearch();
            }}
          />
          <Button
            size="sm"
            className="h-8 px-3 text-[12px]"
            disabled={searching || !searchQ.trim()}
            onClick={() => void runSearch()}
          >
            {searching ? "查找中…" : "立即查找"}
          </Button>
        </div>

        {searchHits ? (
          <div className="mt-2.5 flex flex-col gap-1.5">
            {searchHits.length === 0 ? (
              <div className="text-[12px] text-muted-foreground py-1">(未找到匹配内容)</div>
            ) : (
              searchHits.map((h) => (
                <div key={h.seq} className="rounded-lg border bg-muted/20 px-2.5 py-1.5 text-[11.5px]">
                  <div className="flex items-center justify-between text-muted-foreground">
                    <span>记录 #{h.seq} · 第 {h.turn_index} 轮</span>
                    <span>{h.session_id || "全局"}</span>
                  </div>
                  <pre className="mt-1 max-h-20 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-foreground/80">
                    {JSON.stringify(h.data ?? h.messages ?? h, null, 0).slice(0, 300)}
                  </pre>
                </div>
              ))
            )}
          </div>
        ) : null}
      </div>
    </div>
  );
}
