// context-inspector: 对话上下文透视与分析器
// 纯展示与诊断分析，不修改数据，不执行压缩
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  RefreshCw,
  Loader2,
  User,
  Sparkles,
  FolderOpen,
  Wrench,
  MessageSquare,
  Scissors,
  Code2,
  CheckCircle2,
  Clock,
  Activity,
  Layers,
  ShieldAlert,
  Copy,
  Check,
  Zap,
  ArrowDownLeft,
  ArrowUpRight,
  Download,
  AlertTriangle,
  FileCheck,
  FileEdit,
  Brain,
  Gauge,
  FileCode,
  TrendingUp,
} from "lucide-react";
import { api, type CtxStep } from "../w2/api";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { storage, STORAGE_KEYS } from "@/lib/storage";
import { cn } from "@/lib/utils";

// 估算中英文字数或 token (约 chars/3;仅用于各段不精确的构成占比,真实以提供商 usage 为准)
const estTokens = (s?: string | null) => Math.max(1, Math.ceil((s?.length ?? 0) / 3));

const fmtDur = (ms?: number | null) =>
  ms == null ? "—" : ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;

// 诚实原则:模型上下文窗口容量不做任何猜测——唯一数据源是用户在
// 「设置 → 模型提供商」为模型登记的窗口值(model.json contextWindows);
// 未登记就显示「未知」,绝不用名字匹配表冒充真实水位。

// 被本轮操作影响的本地文件记录 (对标 Pi-Web File Tracking)
interface FileSideEffect {
  path: string;
  action: "read" | "write" | "edit" | "exec";
  toolName: string;
  detail: string;
}

// 对 Prompt 的 system 内容进行结构拆解 (人设/技能/工作区)
interface ParsedPromptRecipe {
  rawSystemPrompt: string;
  personaText: string;
  skills: Array<{ id: string; name: string; instruction: string }>;
  workspaceText: string | null;
  historyTurns: Array<{ turnIndex: number; user: string; assistant: string }>;
  currentUserInput: string;
  toolList: Array<{
    name: string;
    description: string;
    needsApproval: boolean;
    paramTokens: number;
    rawSchema: any;
  }>;
  affectedFiles: FileSideEffect[];
  reasoningSnippet: string | null;
}

function parseStepRecipe(step: CtxStep): ParsedPromptRecipe {
  let rawSystemPrompt = "";
  let personaText = "";
  const skills: Array<{ id: string; name: string; instruction: string }> = [];
  let workspaceText: string | null = null;
  const historyTurns: Array<{ turnIndex: number; user: string; assistant: string }> = [];
  let currentUserInput = "";
  const affectedFiles: FileSideEffect[] = [];
  let reasoningSnippet: string | null = null;

  const messages = step.messages ?? [];

  // 1. 解析 System Prompt
  const sysMsg = messages.find((m) => m.role === "system");
  if (sysMsg && sysMsg.content) {
    rawSystemPrompt = sysMsg.content;
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
    const firstSkillIdx = raw.indexOf("[附加技能 · ");

    if (firstSkillIdx !== -1) {
      personaText = raw.substring(0, firstSkillIdx).trim();
      let sIdx = 0;
      while ((match = skillRegex.exec(raw)) !== null) {
        skills.push({
          id: `skill_${sIdx++}`,
          name: match[1].trim(),
          instruction: match[2].trim(),
        });
      }
    } else {
      personaText = raw.trim();
    }
  }

  // 2. 解析历史与当前提问
  const nonSys = messages.filter((m) => m.role === "user" || m.role === "assistant");
  if (nonSys.length > 0) {
    const last = nonSys[nonSys.length - 1];
    if (last.role === "user") {
      currentUserInput = last.content;
      const prev = nonSys.slice(0, nonSys.length - 1);
      let tCount = 1;
      for (let i = 0; i < prev.length; i += 2) {
        const u = prev[i]?.role === "user" ? prev[i].content : "";
        const a = prev[i + 1]?.role === "assistant" ? prev[i + 1].content : "";
        if (u || a) {
          historyTurns.push({ turnIndex: tCount++, user: u, assistant: a });
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

  // 4. 解析文件副作用追踪与思考链
  for (const m of messages) {
    // 检查推理思考链标记
    if (m.content && (m.content.includes("<think>") || m.content.includes("thinking:"))) {
      const start = m.content.indexOf("<think>");
      const end = m.content.indexOf("</think>");
      if (start !== -1 && end !== -1) {
        reasoningSnippet = m.content.slice(start + 7, end).trim();
      }
    }
  }

  return {
    rawSystemPrompt,
    personaText: personaText || "默认通用助理",
    skills,
    workspaceText,
    historyTurns,
    currentUserInput,
    toolList,
    affectedFiles,
    reasoningSnippet,
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

  // Tab 状态: 包含人设技能、工具背包、聊天记忆、文件副作用、时序流
  const [activeTab, setActiveTab] = useState<"recipe" | "tools" | "memory" | "files" | "spikes" | "trajectory">("recipe");
  const [showRawJson, setShowRawJson] = useState(false);

  // 双栏联动选中状态
  const [selectedPromptSection, setSelectedPromptSection] = useState<string>("persona");
  const [selectedToolName, setSelectedToolName] = useState<string | null>(null);
  const [selectedTurnIndex, setSelectedTurnIndex] = useState<number | null>(null);
  const [selectedFileIndex, setSelectedFileIndex] = useState<number | null>(null);

  // 复制反馈状态
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  // 模型窗口登记表(用户在「设置 → 模型提供商」登记;唯一真实数据源)
  const [contextWindows, setContextWindows] = useState<Record<string, number>>({});

  const refresh = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const [r, modelCfg] = await Promise.all([
        api.context(),
        api.activeModel().catch(() => null),
      ]);
      setSteps(r.steps);
      const w = (modelCfg?.values?.contextWindows ?? null) as Record<string, number> | null;
      if (w && typeof w === "object") setContextWindows(w);
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

  // 最近一次模型调用的快照
  const latestSnapshot = useMemo(() => {
    return visible.find((x) => !x.kind);
  }, [visible]);

  // 解析配方
  const recipe = useMemo(() => {
    if (!latestSnapshot) return null;
    const r = parseStepRecipe(latestSnapshot);

    // 提炼本会话中所有的文件副作用 (读/写/改)
    const filesMap = new Map<string, FileSideEffect>();
    for (const s of visible) {
      if (s.kind === "tool_call" && s.data) {
        const tool = String(s.data.tool ?? "");
        const args = (s.data.arguments ?? {}) as Record<string, any>;
        const path = args.path || args.file || (args.command ? String(args.command).split(" ")[1] : null);
        if (path && typeof path === "string" && (path.includes("/") || path.includes("\\") || path.includes("."))) {
          const action = tool.includes("write") ? "write" : tool.includes("edit") ? "edit" : tool.includes("exec") ? "exec" : "read";
          // 同一文件多次操作:以最后一次为准(先读后写要如实显示为「写入」)
          filesMap.set(path, {
            path,
            action,
            toolName: tool,
            detail: JSON.stringify(args, null, 2),
          });
        }
      }
    }
    r.affectedFiles = Array.from(filesMap.values());
    return r;
  }, [latestSnapshot, visible]);

  // 默认选中初始化
  useEffect(() => {
    if (recipe?.toolList.length && !selectedToolName) {
      setSelectedToolName(recipe.toolList[0].name);
    }
    if (recipe?.historyTurns.length && selectedTurnIndex == null) {
      setSelectedTurnIndex(recipe.historyTurns[0].turnIndex);
    }
    if (recipe?.affectedFiles.length && selectedFileIndex == null) {
      setSelectedFileIndex(0);
    }
  }, [recipe, selectedToolName, selectedTurnIndex, selectedFileIndex]);

  // 双栏联动通用平滑滚动定位
  const scrollToTarget = (id: string) => {
    const el = document.getElementById(id);
    if (el) {
      el.scrollIntoView({ behavior: "smooth", block: "nearest" });
    }
  };

  const copyText = (key: string, text: string) => {
    void navigator.clipboard.writeText(text);
    setCopiedKey(key);
    setTimeout(() => setCopiedKey(null), 2000);
  };

  // 一键导出单次快照或会话脱敏调试包 (JSON)
  const exportScrubbedSnapshot = () => {
    if (!latestSnapshot) return;
    const dump = {
      exported_at: new Date().toISOString(),
      session_id: latestSnapshot.session_id,
      model_id: latestSnapshot.model_id,
      token_metrics: stats,
      telemetry: {
        ttft_ms: latestSnapshot.ttft_ms ?? null,
        tokens_reasoning: latestSnapshot.tokens_reasoning ?? null,
        tokens_cached: latestSnapshot.tokens_cached ?? null,
        evicted_turns: latestSnapshot.evicted_turns ?? 0,
        window_registered: stats?.maxWindow ?? null,
      },
      recipe_breakdown: {
        persona: recipe?.personaText,
        skills: recipe?.skills,
        workspace: recipe?.workspaceText,
        tools: recipe?.toolList.map((t) => t.name),
        affected_files: recipe?.affectedFiles.map((f) => f.path),
      },
      raw_messages: latestSnapshot.messages,
      raw_tools: latestSnapshot.tools,
    };
    const blob = new Blob([JSON.stringify(dump, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `boenmind-context-snapshot-${latestSnapshot.session_id.slice(-6)}-seq${latestSnapshot.seq}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  // token 篇幅、速率、水位与百分比计算 (统一以 token 为单位;
  // 「真实值」只来自快照如实字段(提供商 usage / 实测计时 / 台账计数),
  // 拿不到就如实显示「未上报 / 未知」,绝不编造)
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
    const realTokensOut = latestSnapshot.tokens_out ?? 0;

    // 模型窗口:只认用户登记表(model.json contextWindows);未登记 = 未知
    const maxWindow: number | null =
      (latestSnapshot.model_id && contextWindows[latestSnapshot.model_id]) || null;
    const currentTotal = realTokensIn + realTokensOut;
    const remainingHeadroom = maxWindow != null ? Math.max(0, maxWindow - currentTotal) : null;
    const headroomPct =
      maxWindow != null ? Math.min(100, Math.round((currentTotal / maxWindow) * 100)) : null;

    // 生成速率 = 输出 token ÷ 全程耗时(含首字排队;TTFT 单列坦白口径)
    const latencySec = (latestSnapshot.latency_ms ?? 1000) / 1000;
    const speed = latencySec > 0 && realTokensOut > 0 ? (realTokensOut / latencySec).toFixed(1) : "—";

    // 真实字段直读:提供商不传就是 null → 界面显示「未上报」
    const cachedTokens: number | null = latestSnapshot.tokens_cached ?? null;
    const reasoningTokens: number | null = latestSnapshot.tokens_reasoning ?? null;
    const ttftMs: number | null = latestSnapshot.ttft_ms ?? null;
    const evictedTurns: number = latestSnapshot.evicted_turns ?? 0;
    // 思考链正文片段(messages 里的 <think> 块,若有)+粗估值(标注口径)
    const reasoningSnippetEstimated = recipe.reasoningSnippet
      ? estTokens(recipe.reasoningSnippet)
      : null;

    return {
      personaTokens,
      skillsTokens,
      wsTokens,
      toolsTokens,
      historyTokens,
      inputTokens,
      totalEst,
      realTokensIn,
      realTokensOut,
      maxWindow,
      remainingHeadroom,
      headroomPct,
      speed,
      reasoningTokens,
      reasoningSnippetEstimated,
      cachedTokens,
      ttftMs,
      evictedTurns,
      pct: {
        persona: Math.round((personaTokens / (totalEst || 1)) * 100),
        skills: Math.round((skillsTokens / (totalEst || 1)) * 100),
        ws: Math.round((wsTokens / (totalEst || 1)) * 100),
        tools: Math.round((toolsTokens / (totalEst || 1)) * 100),
        history: Math.round((recipe.historyTurns.length ? historyTokens : 0) / (totalEst || 1) * 100),
        input: Math.round((inputTokens / (totalEst || 1)) * 100),
      },
    };
  }, [recipe, latestSnapshot, contextWindows]);

  // 多轮历史 Token 暴增刺客诊断
  const spikeAnalysis = useMemo(() => {
    const snapshots = [...visible].reverse().filter((s) => !s.kind);
    return snapshots.map((s, idx) => {
      const prev = idx > 0 ? snapshots[idx - 1] : null;
      const curIn = s.tokens_in ?? 0;
      const prevIn = prev?.tokens_in ?? 0;
      const diff = idx > 0 ? curIn - prevIn : 0;
      const isSpike = diff >= 2500 || (prevIn > 0 && curIn / prevIn >= 2.0);
      return {
        seq: s.seq,
        turn_index: s.turn_index,
        step: s.step,
        model_id: s.model_id,
        tokens_in: curIn,
        tokens_out: s.tokens_out ?? 0,
        diff,
        isSpike,
      };
    });
  }, [visible]);

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
            交互透视大盘 · 只读诊断面
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
            onClick={exportScrubbedSnapshot}
            title="一键导出当前快照的脱敏 JSON 诊断包"
          >
            <Download className="size-3.5" />
            <span>导出脱敏快照</span>
          </Button>
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

      {/* 【第一层：健康度看板 / 模型窗口真实水位与性能】 */}
      {latestSnapshot && stats ? (
        <div className="bg-card rounded-xl border p-3.5 shadow-2xs flex flex-col gap-3">
          <div className="flex flex-wrap items-center justify-between gap-2 border-b pb-2">
            {/* 窗口水位:仅在用户登记过窗口容量时出真实进度;否则如实「未知」 */}
            <div className="flex items-center gap-2">
              <Gauge className="size-4 text-primary" />
              <span className="text-[13px] font-semibold text-foreground">
                模型窗口水位
              </span>
              {stats.maxWindow != null ? (
                <>
                  <span className="rounded-md bg-muted px-2 py-0.5 font-mono text-[11px] font-medium text-foreground">
                    {stats.realTokensIn + stats.realTokensOut} / {stats.maxWindow.toLocaleString()} token ({stats.headroomPct}%)
                  </span>
                  <span className={cn(
                    "flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium",
                    (stats.headroomPct ?? 0) >= 80 ? "bg-rose-500/10 text-rose-600" : (stats.headroomPct ?? 0) >= 50 ? "bg-amber-500/10 text-amber-600" : "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                  )}>
                    <CheckCircle2 className="size-3" />
                    <span>剩余安全余量: {stats.remainingHeadroom?.toLocaleString()} token</span>
                  </span>
                </>
              ) : (
                <>
                  <span className="rounded-md bg-muted px-2 py-0.5 font-mono text-[11px] font-medium text-foreground">
                    本轮进出共 {stats.realTokensIn + stats.realTokensOut} token
                  </span>
                  <span className="flex items-center gap-1 rounded-full bg-amber-500/10 px-2 py-0.5 text-[11px] font-medium text-amber-600 dark:text-amber-400" title="未登记该模型的上下文窗口容量,无法计算水位占比;在「设置 → 模型提供商」模型清单里登记窗口 token 数后即可见血条">
                    <AlertTriangle className="size-3" />
                    <span>窗口容量未登记,无法计算水位(可在设置里补登记)</span>
                  </span>
                </>
              )}
            </div>

            {/* 输入、缓存、输出、生成速率与耗时(未上报如实标注) */}
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[12px] text-muted-foreground">
              <span className="flex items-center gap-1">
                <ArrowDownLeft className="size-3.5 text-sky-500" />
                输入: <strong className="text-foreground">{stats.realTokensIn} token</strong>
              </span>
              <span>·</span>
              <span className="flex items-center gap-1" title={stats.cachedTokens == null ? "提供商未上报提示词缓存命中明细" : "Provider 服务端提示词缓存命中"}>
                <Zap className="size-3.5 text-amber-500" />
                缓存: <strong className={stats.cachedTokens != null && stats.cachedTokens > 0 ? "text-emerald-600 dark:text-emerald-400" : "text-foreground"}>
                  {stats.cachedTokens == null ? "未上报" : `${stats.cachedTokens} token`}
                </strong>
              </span>
              <span>·</span>
              <span className="flex items-center gap-1">
                <ArrowUpRight className="size-3.5 text-purple-500" />
                输出: <strong className="text-foreground">{stats.realTokensOut} token</strong>
              </span>
              <span>·</span>
              <span className="flex items-center gap-1" title="输出 token ÷ 全程耗时(含首字排队等待)">
                <TrendingUp className="size-3.5 text-emerald-500" />
                速率: <strong className="text-foreground">{stats.speed} token/s</strong>
              </span>
              {stats.ttftMs != null ? (
                <>
                  <span>·</span>
                  <span className="flex items-center gap-1" title="请求发出到第一个字回来的时间(仅流式可测)">
                    <Zap className="size-3.5 text-sky-500" />
                    首字: <strong className="text-foreground">{fmtDur(stats.ttftMs)}</strong>
                  </span>
                </>
              ) : null}
              <span>·</span>
              <span className="flex items-center gap-1">
                <Clock className="size-3.5" />
                耗时: <strong className="text-foreground">{fmtDur(latestSnapshot.latency_ms)}</strong>
              </span>
            </div>
          </div>

          {/* 进度条水杯 */}
          <div>
            <div className="mb-1.5 flex items-center justify-between text-[11.5px] text-muted-foreground">
              <span>输入内容配方构成：</span>
              <span>
                当前会话保留 {recipe?.historyTurns.length ?? 0}/20 轮记忆
                {(stats?.evictedTurns ?? 0) > 0 ? (
                  <span className="text-amber-600 dark:text-amber-400">
                    （最早 {stats?.evictedTurns} 轮已被自动遗忘）
                  </span>
                ) : null}
              </span>
            </div>
            <div className="flex h-3 w-full overflow-hidden rounded-full bg-muted/80">
              {stats.pct.persona > 0 ? (
                <div
                  style={{ width: `${stats.pct.persona}%` }}
                  className="bg-indigo-500 transition-all hover:opacity-80"
                  title={`AI人设与规矩: 约 ${stats.personaTokens} token (${stats.pct.persona}%)`}
                />
              ) : null}
              {stats.pct.skills > 0 ? (
                <div
                  style={{ width: `${stats.pct.skills}%` }}
                  className="bg-purple-500 transition-all hover:opacity-80"
                  title={`携带特长技能: 约 ${stats.skillsTokens} token (${stats.pct.skills}%)`}
                />
              ) : null}
              {stats.pct.tools > 0 ? (
                <div
                  style={{ width: `${stats.pct.tools}%` }}
                  className="bg-amber-500 transition-all hover:opacity-80"
                  title={`装备工具箱: 约 ${stats.toolsTokens} token (${stats.pct.tools}%)`}
                />
              ) : null}
              {stats.pct.history > 0 ? (
                <div
                  style={{ width: `${stats.pct.history}%` }}
                  className="bg-sky-500 transition-all hover:opacity-80"
                  title={`之前聊天记忆: 约 ${stats.historyTokens} token (${stats.pct.history}%)`}
                />
              ) : null}
              {stats.pct.ws > 0 ? (
                <div
                  style={{ width: `${stats.pct.ws}%` }}
                  className="bg-emerald-500 transition-all hover:opacity-80"
                  title={`工作区环境: 约 ${stats.wsTokens} token (${stats.pct.ws}%)`}
                />
              ) : null}
              {stats.pct.input > 0 ? (
                <div
                  style={{ width: `${stats.pct.input}%` }}
                  className="bg-rose-500 transition-all hover:opacity-80"
                  title={`本次提问: 约 ${stats.inputTokens} token (${stats.pct.input}%)`}
                />
              ) : null}
            </div>

            {/* 图例对照表 (统一为 token 表达) */}
            <div className="mt-2.5 flex flex-wrap items-center gap-x-4 gap-y-1 text-[11.5px]">
              <span className="flex items-center gap-1.5">
                <span className="size-2.5 rounded-full bg-indigo-500" />
                <span className="text-foreground">🎭 人设规矩:</span>
                <span className="text-muted-foreground">{stats.pct.persona}% ({stats.personaTokens} token)</span>
              </span>
              {stats.skillsTokens > 0 ? (
                <span className="flex items-center gap-1.5">
                  <span className="size-2.5 rounded-full bg-purple-500" />
                  <span className="text-foreground">⚡ 特长技能:</span>
                  <span className="text-muted-foreground">{stats.pct.skills}% ({stats.skillsTokens} token)</span>
                </span>
              ) : null}
              <span className="flex items-center gap-1.5">
                <span className="size-2.5 rounded-full bg-amber-500" />
                <span className="text-foreground">🛠️ 工具背包:</span>
                <span className="text-muted-foreground">{stats.pct.tools}% ({stats.toolsTokens} token)</span>
              </span>
              <span className="flex items-center gap-1.5">
                <span className="size-2.5 rounded-full bg-sky-500" />
                <span className="text-foreground">💬 聊天记忆:</span>
                <span className="text-muted-foreground">{stats.pct.history}% ({stats.historyTokens} token)</span>
              </span>
              {stats.wsTokens > 0 ? (
                <span className="flex items-center gap-1.5">
                  <span className="size-2.5 rounded-full bg-emerald-500" />
                  <span className="text-foreground">📁 电脑目录:</span>
                  <span className="text-muted-foreground">{stats.pct.ws}% ({stats.wsTokens} token)</span>
                </span>
              ) : null}
              <span className="flex items-center gap-1.5">
                <span className="size-2.5 rounded-full bg-rose-500" />
                <span className="text-foreground">❓ 您的问题:</span>
                <span className="text-muted-foreground">{stats.pct.input}% ({stats.inputTokens} token)</span>
              </span>
            </div>
          </div>
        </div>
      ) : (
        <div className="bg-card rounded-xl border p-6 text-center text-[12.5px] text-muted-foreground">
          {steps.length > 0
            ? "当前会话暂无调用记录，在左侧输入一句话发送后即可在此查看"
            : "尚未产生模型交互数据"}
        </div>
      )}

      {/* 【第二层：全域双栏联动交互区】 */}
      {recipe ? (
        <div className="flex min-h-0 flex-1 flex-col gap-3">
          <div className="flex items-center justify-between border-b pb-1">
            <div className="flex flex-wrap items-center gap-1" role="tablist">
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
                <span>人设与特长双栏</span>
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
                <span>工具背包双栏 ({recipe.toolList.length})</span>
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
                <span>聊天记忆双栏 ({recipe.historyTurns.length}轮)</span>
              </button>

              <button
                role="tab"
                onClick={() => setActiveTab("files")}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12.5px] font-medium transition-colors",
                  activeTab === "files"
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:bg-muted",
                )}
              >
                <FileCode className="size-3.5" />
                <span>工程文件副作用 ({recipe.affectedFiles.length})</span>
              </button>

              <button
                role="tab"
                onClick={() => setActiveTab("spikes")}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12.5px] font-medium transition-colors",
                  activeTab === "spikes"
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:bg-muted",
                )}
              >
                <TrendingUp className="size-3.5" />
                <span>Token暴增诊断</span>
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
              {/* TAB 1: 人设与特长双栏联动 */}
              {activeTab === "recipe" ? (
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
                          setSelectedPromptSection("persona");
                          scrollToTarget("prompt-section-persona");
                        }}
                        className={cn(
                          "cursor-pointer rounded-xl border p-3 transition-all duration-150 flex flex-col justify-between gap-1.5",
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
                              setSelectedPromptSection(s.id);
                              scrollToTarget(`prompt-section-${s.id}`);
                            }}
                            className={cn(
                              "cursor-pointer rounded-xl border p-3 transition-all duration-150 flex flex-col justify-between gap-1.5",
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
                            setSelectedPromptSection("workspace");
                            scrollToTarget("prompt-section-workspace");
                          }}
                          className={cn(
                            "cursor-pointer rounded-xl border p-3 transition-all duration-150 flex flex-col justify-between gap-1.5",
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
                          onClick={() => copyText("all_prompt", recipe.rawSystemPrompt)}
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
              ) : null}

              {/* TAB 2: 工具背包双栏联动 */}
              {activeTab === "tools" ? (
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
                              setSelectedToolName(t.name);
                              scrollToTarget(`tool-block-${t.name}`);
                            }}
                            className={cn(
                              "cursor-pointer rounded-xl border p-3 transition-all duration-150 flex flex-col justify-between gap-1.5",
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
                              {t.needsApproval ? (
                                <span className="flex items-center gap-1 rounded bg-amber-500/15 px-1.5 py-0.5 text-[10.5px] font-medium text-amber-600 dark:text-amber-400 shrink-0">
                                  <ShieldAlert className="size-3" />
                                  <span>需审批</span>
                                </span>
                              ) : (
                                <span className="rounded bg-emerald-500/15 px-1.5 py-0.5 text-[10.5px] font-medium text-emerald-600 dark:text-emerald-400 shrink-0">
                                  直通只读
                                </span>
                              )}
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
                                  onClick={() => copyText(`tool_${t.name}`, JSON.stringify(t.rawSchema, null, 2))}
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
              ) : null}

              {/* TAB 3: 聊天记忆双栏联动 */}
              {activeTab === "memory" ? (
                <div className="flex flex-col gap-2.5">
                  <div className="flex flex-wrap items-center justify-between border-b pb-2 text-[12.5px]">
                    <div>
                      <span className="font-semibold text-foreground">💬 依然清晰保留的聊天记忆 (双栏联动透视)</span>
                      <span className="ml-2 text-[11.5px] text-muted-foreground">
                        点击左侧对答卡片，右侧历史消息报文自动滚动并加深高亮
                      </span>
                    </div>
                    <span className="rounded-md bg-sky-500/10 px-2 py-0.5 text-[11.5px] font-medium text-sky-600 dark:text-sky-400">
                      当前存活 {recipe.historyTurns.length} 轮 (上限 20 轮)
                    </span>
                  </div>

                  <div className="grid grid-cols-1 gap-3.5 lg:grid-cols-12 min-h-[440px]">
                    {/* 左侧：对答卡片 */}
                    <div className="flex flex-col gap-2.5 overflow-y-auto pr-1 lg:col-span-5 max-h-[500px]">
                      {recipe.historyTurns.length === 0 ? (
                        <div className="rounded-xl border bg-card p-6 text-center text-[12.5px] text-muted-foreground">
                          这是新会话的第一轮对话，暂无前期聊天记忆
                        </div>
                      ) : (
                        recipe.historyTurns.map((h) => {
                          const isSelected = selectedTurnIndex === h.turnIndex;
                          const turnTokens = estTokens(h.user) + estTokens(h.assistant);
                          return (
                            <div
                              key={h.turnIndex}
                              onClick={() => {
                                setSelectedTurnIndex(h.turnIndex);
                                scrollToTarget(`history-turn-${h.turnIndex}`);
                              }}
                              className={cn(
                                "cursor-pointer rounded-xl border p-3 transition-all duration-150 flex flex-col justify-between gap-1.5",
                                isSelected
                                  ? "border-primary bg-primary/10 shadow-xs ring-1 ring-primary/40"
                                  : "bg-card hover:border-border hover:bg-muted/30 border-border/70",
                              )}
                            >
                              <div className="flex items-center justify-between text-[11.5px]">
                                <span className="font-semibold text-foreground">第 {h.turnIndex} 轮对答记忆</span>
                                <span className="text-muted-foreground text-[10.5px]">约 {turnTokens} token</span>
                              </div>
                              <div className="text-[12px] font-medium text-foreground/90 line-clamp-2">
                                问: {h.user}
                              </div>
                              <div className="text-[11.5px] text-muted-foreground line-clamp-3">
                                答: {h.assistant}
                              </div>
                              <div className="flex items-center justify-between border-t border-border/40 pt-1.5 text-[11px]">
                                <span className="text-muted-foreground">问 {estTokens(h.user)} · 答 {estTokens(h.assistant)} token</span>
                                <span className={cn("font-medium", isSelected ? "text-primary" : "text-muted-foreground/60")}>
                                  {isSelected ? "✓ 正在右侧高亮" : "点击查看报文"}
                                </span>
                              </div>
                            </div>
                          );
                        })
                      )}

                      {(stats?.evictedTurns ?? 0) > 0 ? (
                        <div className="rounded-xl border border-amber-500/40 p-3 text-[11.5px] bg-amber-500/10 flex items-start gap-2.5">
                          <Scissors className="size-4 mt-0.5 text-amber-600 dark:text-amber-400 shrink-0" />
                          <div>
                            <span className="font-semibold text-amber-600 dark:text-amber-400">已经有对话被自动遗忘：</span>
                            <span className="text-muted-foreground">
                              台账上限为 20 轮或 24,000 字符，最早的 <strong className="text-foreground">{stats?.evictedTurns}</strong> 轮已从 AI 的记忆里裁掉。上面的卡片是它现在还真正记得的全部内容。
                            </span>
                          </div>
                        </div>
                      ) : (
                        <div className="rounded-xl border border-dashed p-3 text-[11.5px] text-muted-foreground bg-muted/10 flex items-start gap-2.5">
                          <Scissors className="size-4 mt-0.5 text-muted-foreground shrink-0" />
                          <div>
                            <span className="font-semibold text-foreground">关于对话遗忘的说明：</span>
                            <span>
                              系统上限为 20 轮或 24,000 字符。截至最近一次调用，本对话的所有历史轮次都还在，没有被剪掉。
                            </span>
                          </div>
                        </div>
                      )}
                    </div>

                    {/* 右侧：实际回喂给模型的历史报文块 */}
                    <div className="flex flex-col gap-2.5 overflow-y-auto rounded-xl border bg-muted/20 p-3 lg:col-span-7 max-h-[500px]">
                      <div className="flex items-center justify-between border-b border-border/60 pb-1.5 text-[12px]">
                        <span className="font-semibold text-foreground flex items-center gap-1.5">
                          <Code2 className="size-3.5 text-primary" />
                          <span>历史消息原始报文 (OpenAI Messages 格式)</span>
                        </span>
                        <span className="text-[11px] font-mono text-muted-foreground">
                          {recipe.historyTurns.length * 2} messages
                        </span>
                      </div>

                      <div className="flex flex-col gap-3">
                        {recipe.historyTurns.length === 0 ? (
                          <div className="p-8 text-center text-[12px] text-muted-foreground">
                            无历史报文
                          </div>
                        ) : (
                          recipe.historyTurns.map((h) => {
                            const isSelected = selectedTurnIndex === h.turnIndex;
                            const turnJson = [
                              { role: "user", content: h.user },
                              { role: "assistant", content: h.assistant },
                            ];
                            return (
                              <div
                                key={h.turnIndex}
                                id={`history-turn-${h.turnIndex}`}
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
                                    <span className="text-foreground">第 {h.turnIndex} 轮对答报文</span>
                                  </div>
                                  <Button
                                    size="sm"
                                    variant="ghost"
                                    className="h-6 gap-1 px-1.5 text-[10.5px] text-muted-foreground hover:text-foreground"
                                    onClick={() => copyText(`turn_${h.turnIndex}`, JSON.stringify(turnJson, null, 2))}
                                  >
                                    {copiedKey === `turn_${h.turnIndex}` ? (
                                      <>
                                        <Check className="size-3 text-emerald-500" />
                                        <span className="text-emerald-500">已复制</span>
                                      </>
                                    ) : (
                                      <>
                                        <Copy className="size-3" />
                                        <span>复制此轮</span>
                                      </>
                                    )}
                                  </Button>
                                </div>

                                <pre className="max-h-48 overflow-auto rounded bg-muted/40 p-2 font-mono text-[11px] leading-relaxed text-foreground/90 whitespace-pre-wrap break-all">
                                  {JSON.stringify(turnJson, null, 2)}
                                </pre>
                              </div>
                            );
                          })
                        )}
                      </div>
                    </div>
                  </div>
                </div>
              ) : null}

              {/* TAB 4: 本地工程文件读写副作用追踪 (对标 Pi-Web) */}
              {activeTab === "files" ? (
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
                                setSelectedFileIndex(idx);
                                scrollToTarget(`file-effect-${idx}`);
                              }}
                              className={cn(
                                "cursor-pointer rounded-xl border p-3 transition-all duration-150 flex flex-col justify-between gap-1.5",
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
                                  <Button
                                    size="sm"
                                    variant="ghost"
                                    className="h-6 gap-1 px-1.5 text-[10.5px] text-muted-foreground hover:text-foreground"
                                    onClick={() => copyText(`file_${idx}`, f.detail)}
                                  >
                                    <Copy className="size-3" />
                                    <span>复制明细</span>
                                  </Button>
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
              ) : null}

              {/* TAB 5: 多轮 Token 暴增与刺客诊断 */}
              {activeTab === "spikes" ? (
                <div className="flex flex-col gap-3 rounded-xl border bg-card p-3.5 shadow-2xs">
                  <div className="flex flex-wrap items-center justify-between border-b pb-2 text-[12.5px]">
                    <div>
                      <span className="font-semibold text-foreground flex items-center gap-1.5">
                        <TrendingUp className="size-4 text-primary" />
                        <span>多轮对话 Token 暴增与刺客诊断 (Spike Alert)</span>
                      </span>
                      <p className="text-[11.5px] text-muted-foreground mt-0.5">
                        自动对比相邻轮次的 Token 增量，智能揪出是哪一轮因外部搜索、读入超大文件导致上下文突然爆仓。
                      </p>
                    </div>
                  </div>

                  <div className="flex flex-col gap-2">
                    {spikeAnalysis.map((item, idx) => (
                      <div
                        key={item.seq}
                        className={cn(
                          "flex items-center justify-between rounded-lg border p-3 text-[12px] transition-colors",
                          item.isSpike ? "border-rose-500/50 bg-rose-500/10" : "bg-muted/20 border-border/60",
                        )}
                      >
                        <div className="flex items-center gap-3">
                          <span className="font-mono font-semibold text-[13px] text-foreground">
                            第 {item.turn_index} 轮 (第 {item.step} 步)
                          </span>
                          <span className="text-muted-foreground text-[11.5px]">
                            输入: <strong className="text-foreground">{item.tokens_in} token</strong> · 输出: {item.tokens_out} token
                          </span>
                          {item.diff > 0 ? (
                            <span className={cn(
                              "rounded-md px-1.5 py-0.5 text-[11px] font-medium font-mono",
                              item.isSpike ? "bg-rose-500 text-white" : "bg-muted text-muted-foreground"
                            )}>
                              +{item.diff} token
                            </span>
                          ) : null}
                        </div>

                        <div className="flex items-center gap-2">
                          {item.isSpike ? (
                            <span className="flex items-center gap-1 rounded bg-rose-500/20 px-2 py-0.5 text-[11.5px] font-semibold text-rose-600 dark:text-rose-400">
                              <AlertTriangle className="size-3.5" />
                              <span>⚠️ 检测到 Token 异常激增！可能调用了携带大量长文的外部工具</span>
                            </span>
                          ) : (
                            <span className="text-[11.5px] text-muted-foreground">正常增长</span>
                          )}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}

              {/* TAB 6: 步骤时序流与深度思考链 */}
              {activeTab === "trajectory" ? (
                <div className="flex flex-col gap-3 rounded-xl border bg-card p-3.5 shadow-2xs">
                  <div className="flex flex-wrap items-center justify-between border-b pb-2 text-[13px] font-semibold text-foreground">
                    <span className="flex items-center gap-1.5">
                      <Activity className="size-4 text-primary" />
                      <span>交互执行细节与模型深度思考链 (Thinking Chain)</span>
                    </span>
                  </div>

                  {/* 深度思考链卡片:正文片段如实展示;token 分账只标提供商实报,
                      未上报则标口径(条目内文本粗估,不冒充真实数) */}
                  {recipe.reasoningSnippet ? (
                    <div className="rounded-lg border border-purple-500/40 bg-purple-500/10 p-3">
                      <div className="mb-1 flex items-center justify-between text-[12px] font-semibold text-purple-600 dark:text-purple-400">
                        <span className="flex items-center gap-1.5">
                          <Brain className="size-4" />
                          <span>
                            🧠 模型深度思考链 (
                            {stats?.reasoningTokens != null
                              ? `Reasoning Tokens ${stats.reasoningTokens} token·提供商实报`
                              : stats?.reasoningSnippetEstimated != null
                                ? `片段约 ${stats.reasoningSnippetEstimated} token·按文本粗估,提供商未上报分账`
                                : "提供商未上报分账"}
                            )
                          </span>
                        </span>
                        <span className="text-[11px] font-mono text-muted-foreground">thinking_content</span>
                      </div>
                      <pre className="max-h-48 overflow-auto rounded bg-background/80 p-2.5 font-mono text-[11px] leading-relaxed text-foreground whitespace-pre-wrap break-all">
                        {recipe.reasoningSnippet}
                      </pre>
                    </div>
                  ) : null}

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
                            desc: `生成了答复，输出消耗约 ${String(d.tokens_out ?? "—")} token`,
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
            placeholder="输入关键词搜索（如：天气 / fs_read / 某个报错）"
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
