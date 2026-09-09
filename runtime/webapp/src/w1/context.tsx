// context-inspector: 对话上下文透视与分析器
// 纯展示与诊断分析，不修改数据，不执行压缩
// #22 拆分:本文件为装配层(状态/派生/顶栏);视图块下放 context/ 目录——
// 健康度看板 HealthBoard、趋势图 TrendChart、双栏联动区 TwoColumnTabs
// (tabs/ 六块)、检索面板 SearchPanel;跨块状态走既有 bus + props。
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { RefreshCw, Loader2, Activity, Download } from "lucide-react";
import { api, type CtxStep } from "../w2/api";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { storage, STORAGE_KEYS } from "@/lib/storage";
import { BM_EVENTS } from "../lib/bus";

// 估算中英文字数或 token (约 chars/3;仅用于各段不精确的构成占比,真实以提供商 usage 为准)
import { estTokens, type FileSideEffect } from "./context/utils";
import { parseStepRecipe } from "./context/recipe";
import { HealthBoard } from "./context/HealthBoard";
import { TrendChart } from "./context/TrendChart";
import { TwoColumnTabs, type ContextTab } from "./context/TwoColumnTabs";
import { SearchPanel } from "./context/SearchPanel";

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
  const [activeTab, setActiveTab] = useState<ContextTab>("recipe");
  const [showRawJson, setShowRawJson] = useState(false);

  // 双栏联动选中状态
  const [selectedPromptSection, setSelectedPromptSection] = useState<string>("persona");
  const [selectedToolName, setSelectedToolName] = useState<string | null>(null);
  const [selectedTurnIndex, setSelectedTurnIndex] = useState<number | null>(null);
  const [selectedFileIndex, setSelectedFileIndex] = useState<number | null>(null);

  // DSH 趋势图与速报状态 (阶段一)
  const [trendGranularity, setTrendGranularity] = useState<"step" | "turn">("step");
  const [trendMode, setTrendMode] = useState<"total" | "delta">("total");
  // 选中项复合键(session:seq)。seq 只在单服务生命周期内单调,跨会话重复,
  // 必须与会话号联合定位唯一快照
  const [selectedStepKey, setSelectedStepKey] = useState<string | null>(null);
  const [hoveredCategory, setHoveredCategory] = useState<string | null>(null);

  // 复制反馈状态
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  // 模型窗口登记表(用户在「设置 → 模型提供商」登记;唯一真实数据源)
  const [contextWindows, setContextWindows] = useState<Record<string, number>>({});

  // P1-31(2026-09-07 架构评审):在途守卫——慢响应期间不再重入,防止
  // 先发后至的 setSteps 用旧数据覆盖新数据(下一 tick 自愈的乱序问题根除)
  const refreshInFlightRef = useRef(false);

  const refresh = useCallback(async () => {
    if (refreshInFlightRef.current) return;
    refreshInFlightRef.current = true;
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
      refreshInFlightRef.current = false;
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const handleNewChat = () => {
      setSelectedStepKey(null);
      void refresh();
    };
    window.addEventListener(BM_EVENTS.chatNew, handleNewChat);
    return () => window.removeEventListener(BM_EVENTS.chatNew, handleNewChat);
  }, [refresh]);

  useEffect(() => {
    if (!auto) return;
    const t = setInterval(() => void refresh(), 8_000);
    return () => clearInterval(t);
  }, [auto, refresh]);

  // P1-32(2026-09-07 架构评审):渲染期不直读 localStorage——首帧惰性
  // 初始化 + 会话切换事件时同步
  const [sid, setSid] = useState(() => storage.get(STORAGE_KEYS.SESSION));
  useEffect(() => {
    const sync = () => setSid(storage.get(STORAGE_KEYS.SESSION));
    window.addEventListener(BM_EVENTS.sessionSwitched, sync);
    window.addEventListener(BM_EVENTS.chatNew, sync);
    return () => {
      window.removeEventListener(BM_EVENTS.sessionSwitched, sync);
      window.removeEventListener(BM_EVENTS.chatNew, sync);
    };
  }, []);

  // 过滤出当前会话并按时间由新到旧:
  // 当开启「仅当前会话」时:
  // 1. 若当前尚未开启会话(新建对话尚未发第一条)或本地无 sid，则严格为空数组，绝不展示其他会话的历史穿透
  // 2. 若有 sid 则严格过滤出匹配该 sid 的步骤
  const visible = useMemo(() => {
    if (onlyCurrent) {
      if (!sid) return [];
      return [...steps.filter((s) => s.session_id === sid)].reverse();
    }
    return [...steps].reverse();
  }, [steps, onlyCurrent, sid]);

  // 最近一次模型调用的快照
  const latestSnapshot = useMemo(() => {
    return visible.find((x) => !x.kind);
  }, [visible]);

  // DSH 时间旅行:趋势图选中的历史步骤快照(null = 跟随最新现场 Live)
  const timeTravelSnapshot = useMemo(() => {
    if (selectedStepKey == null) return null;
    const [selSession, selSeq] = selectedStepKey.split(":");
    const found = visible.find(
      (x) => !x.kind && x.seq === Number(selSeq) && x.session_id === selSession,
    );
    return found ?? null;
  }, [visible, selectedStepKey]);
  const isTimeTraveling = timeTravelSnapshot != null && timeTravelSnapshot !== latestSnapshot;

  // 解析配方(时间旅行时以选中的历史步骤为透视对象;Live 模式跟随最新)
  const recipe = useMemo(() => {
    const focusSnapshot = timeTravelSnapshot ?? latestSnapshot;
    if (!focusSnapshot) return null;
    const r = parseStepRecipe(focusSnapshot);

    // 提炼本会话中所有的文件副作用 (读/写/改)
    const filesMap = new Map<string, FileSideEffect>();
    for (const s of visible) {
      if (s.kind === "tool_call" && s.data) {
        const tool = String(s.data.tool ?? "");
        const args = (s.data.arguments ?? {}) as Record<string, any>;
        const path = args.path || args.file || (args.command ? String(args.command).split(" ")[1] : null);
        if (path && typeof path === "string" && (path.includes("/") || path.includes("\\") || path.includes("."))) {
          const action = tool.includes("write") ? "write" : tool.includes("edit") ? "edit" : tool.includes("exec") ? "exec" : "read";

          // 代码行数净值统计 (对标 DSH FileCard +N/-M):
          // write=整文行数全部计入新增;edit=old/new 字符串差量统计
          let linesAdded: number | undefined;
          let linesRemoved: number | undefined;
          if (action === "write") {
            const content = String(args.content ?? "");
            if (content) linesAdded = content.split("\n").length;
          } else if (action === "edit") {
            const oldStr = String(args.old_string ?? "");
            const newStr = String(args.new_string ?? "");
            if (oldStr || newStr) {
              linesAdded = newStr ? newStr.split("\n").length : 0;
              linesRemoved = oldStr ? oldStr.split("\n").length : 0;
            }
          }

          // 同一文件多次操作:以最后一次为准(先读后写要如实显示为「写入」),
          // 行数净值同样取最近一次操作的差量
          filesMap.set(path, {
            path,
            action,
            toolName: tool,
            detail: JSON.stringify(args, null, 2),
            linesAdded,
            linesRemoved,
          });
        }
      }
    }
    r.affectedFiles = Array.from(filesMap.values());
    return r;
  }, [timeTravelSnapshot, latestSnapshot, visible]);

  // 退出时间旅行，回到最新现场
  const exitTimeTravel = () => setSelectedStepKey(null);

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
  // 时间旅行时以历史快照的实报为口径;Live 模式跟随最新
  // React Compiler 无法自动保留该复杂派生 memo(引用多快照分支 + 循环派生),
  // 手写 useMemo 为既有正确形态——保留手工 memo 是刻意选型,不是编译器可替代。
  // eslint-disable-next-line react-hooks/preserve-manual-memoization
  const stats = useMemo(() => {
    if (!recipe || !latestSnapshot) return null;
    const focusSnapshot = timeTravelSnapshot ?? latestSnapshot;
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
    const realTokensIn = focusSnapshot.tokens_in ?? totalEst;
    const realTokensOut = focusSnapshot.tokens_out ?? 0;

    // 模型窗口:只认用户登记表(model.json contextWindows);未登记 = 未知
    const maxWindow: number | null =
      (focusSnapshot.model_id && contextWindows[focusSnapshot.model_id]) || null;
    const currentTotal = realTokensIn + realTokensOut;
    const remainingHeadroom = maxWindow != null ? Math.max(0, maxWindow - currentTotal) : null;
    const headroomPct =
      maxWindow != null ? Math.min(100, Math.round((currentTotal / maxWindow) * 100)) : null;

    // 生成速率 = 输出 token ÷ 全程耗时(含首字排队;TTFT 单列坦白口径)
    const latencySec = (focusSnapshot.latency_ms ?? 1000) / 1000;
    const speed = latencySec > 0 && realTokensOut > 0 ? (realTokensOut / latencySec).toFixed(1) : "—";

    // 真实字段直读:提供商不传就是 null → 界面显示「未上报」
    const cachedTokens: number | null = focusSnapshot.tokens_cached ?? null;
    const reasoningTokens: number | null = focusSnapshot.tokens_reasoning ?? null;
    const ttftMs: number | null = focusSnapshot.ttft_ms ?? null;
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
  }, [recipe, timeTravelSnapshot, latestSnapshot, contextWindows]);

  // DSH 时序趋势序列 (正序按时间排列，按步骤或按轮次)
  const trendItems = useMemo(() => {
    const snapshots = [...visible].reverse().filter((s) => !s.kind);
    if (trendGranularity === "step") {
      return snapshots.map((s) => {
        const r = parseStepRecipe(s);
        const pTok = estTokens(r.personaText);
        const skTok = r.skills.reduce((sum, sk) => sum + estTokens(sk.instruction), 0);
        const wsTok = r.workspaceText ? estTokens(r.workspaceText) : 0;
        const toolTok = r.toolList.reduce((sum, t) => sum + t.paramTokens, 0);
        const histTok = r.historyTurns.reduce(
          (sum, h) => sum + estTokens(h.user) + estTokens(h.assistant),
          0,
        );
        const inTok = estTokens(r.currentUserInput);
        const total = pTok + skTok + wsTok + toolTok + histTok + inTok || 1;

        // 提炼三行速报
        const question = r.currentUserInput || "(无显式用户输入)";
        // 提取该步前最后一条非系统消息(作为进入内容)
        const lastInput = r.historyTurns.length > 0 ? r.historyTurns[r.historyTurns.length - 1].assistant : "(首轮无前置输入)";
        const responseSummary = s.status === "error" ? `[失败: ${s.error_code ?? "未知错误"}]` : s.tokens_out ? `回复约 ${s.tokens_out} token` : "完成本次调用";

        return {
          id: `${s.session_id}:${s.seq}`,
          label: `R${s.turn_index} S${s.step}`,
          turn_index: s.turn_index,
          step: s.step,
          tokens_in: s.tokens_in ?? total,
          tokens_out: s.tokens_out ?? 0,
          evicted_turns: s.evicted_turns ?? 0,
          pTok,
          skTok,
          wsTok,
          toolTok,
          histTok,
          inTok,
          total,
          question,
          lastInput,
          responseSummary,
        };
      });
    } else {
      // 聚合按回合 (Turn):键 = 会话 + 回合号(跨会话同号回合不合并)
      const turnMap = new Map<string, CtxStep[]>();
      for (const s of snapshots) {
        const key = `${s.session_id}:${s.turn_index}`;
        const arr = turnMap.get(key) ?? [];
        arr.push(s);
        turnMap.set(key, arr);
      }
      return Array.from(turnMap.entries()).map(([key, list]) => {
        const last = list[list.length - 1];
        const r = parseStepRecipe(last);
        const pTok = estTokens(r.personaText);
        const skTok = r.skills.reduce((sum, sk) => sum + estTokens(sk.instruction), 0);
        const wsTok = r.workspaceText ? estTokens(r.workspaceText) : 0;
        const toolTok = r.toolList.reduce((sum, t) => sum + t.paramTokens, 0);
        const histTok = r.historyTurns.reduce(
          (sum, h) => sum + estTokens(h.user) + estTokens(h.assistant),
          0,
        );
        const inTok = estTokens(r.currentUserInput);
        const total = pTok + skTok + wsTok + toolTok + histTok + inTok || 1;

        const maxIn = Math.max(...list.map((x) => x.tokens_in ?? 0));
        const sumOut = list.reduce((sum, x) => sum + (x.tokens_out ?? 0), 0);
        const turn = last.turn_index;

        return {
          id: key,
          label: `第 ${turn} 轮`,
          turn_index: turn,
          step: list.length,
          tokens_in: maxIn || total,
          tokens_out: sumOut,
          evicted_turns: last.evicted_turns ?? 0,
          pTok,
          skTok,
          wsTok,
          toolTok,
          histTok,
          inTok,
          total,
          question: r.currentUserInput || "(无显式用户输入)",
          lastInput: `本轮共执行 ${list.length} 个步骤`,
          responseSummary: `累计生成 ${sumOut} token`,
        };
      });
    }
  }, [visible, trendGranularity]);

  // 当前选中的步骤信息
  const activeTrendStep = useMemo(() => {
    if (!trendItems.length) return null;
    if (selectedStepKey != null) {
      const found = trendItems.find((x) => x.id === selectedStepKey);
      if (found) return found;
    }
    return trendItems[trendItems.length - 1];
  }, [trendItems, selectedStepKey]);

  // 最大高度缩放基准
  const maxTrendTokens = useMemo(() => {
    return Math.max(1, ...trendItems.map((x) => x.tokens_in + x.tokens_out));
  }, [trendItems]);

  // Delta 增量模式:相邻步相对上一项的 token 差值(向上增长/向下释放)
  const deltaTrendItems = useMemo(() => {
    return trendItems.map((item, idx) => {
      const prev = idx > 0 ? trendItems[idx - 1] : null;
      const cur = item.tokens_in + item.tokens_out;
      const prevTotal = prev ? prev.tokens_in + prev.tokens_out : 0;
      const delta = cur - prevTotal;
      return {
        id: item.id,
        label: item.label,
        delta,
        // 向下(负值=压缩/裁剪释放)取绝对值渲染下探柱
        isNegative: delta < 0,
        magnitude: Math.abs(delta),
        tokens_in: item.tokens_in,
        tokens_out: item.tokens_out,
      };
    });
  }, [trendItems]);
  const maxDeltaMagnitude = useMemo(() => {
    return Math.max(1, ...deltaTrendItems.map((x) => x.magnitude));
  }, [deltaTrendItems]);

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
    <div className="flex min-h-0 flex-1 flex-col gap-3.5 overflow-y-auto px-4 pb-4">
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
        <HealthBoard
          stats={stats}
          recipe={recipe}
          latestSnapshot={latestSnapshot}
          isTimeTraveling={isTimeTraveling}
          timeTravelSnapshot={timeTravelSnapshot}
          onExitTimeTravel={exitTimeTravel}
          hoveredCategory={hoveredCategory}
          onHoverCategory={setHoveredCategory}
        />
      ) : (
        <div className="bg-card rounded-xl border p-6 text-center text-[12.5px] text-muted-foreground">
          {steps.length > 0
            ? "当前会话暂无调用记录，在左侧输入一句话发送后即可在此查看"
            : "尚未产生模型交互数据"}
        </div>
      )}

      {/* 【DSH 视觉化吸收：时序堆叠演进趋势图 + 单步三行白话速报】 */}
      {trendItems.length > 0 ? (
        <TrendChart
          trendItems={trendItems}
          deltaTrendItems={deltaTrendItems}
          maxTrendTokens={maxTrendTokens}
          maxDeltaMagnitude={maxDeltaMagnitude}
          trendMode={trendMode}
          onTrendModeChange={setTrendMode}
          trendGranularity={trendGranularity}
          onTrendGranularityChange={setTrendGranularity}
          activeTrendStep={activeTrendStep}
          onSelectStep={setSelectedStepKey}
          hoveredCategory={hoveredCategory}
        />
      ) : null}

      {/* 【第二层：全域双栏联动交互区】 */}
      {recipe ? (
        <TwoColumnTabs
          activeTab={activeTab}
          onTabChange={setActiveTab}
          showRawJson={showRawJson}
          onToggleRawJson={() => setShowRawJson(!showRawJson)}
          recipe={recipe}
          latestSnapshot={latestSnapshot ?? null}
          stats={stats}
          visible={visible}
          selectedPromptSection={selectedPromptSection}
          onSelectSection={setSelectedPromptSection}
          selectedToolName={selectedToolName}
          onSelectTool={setSelectedToolName}
          selectedTurnIndex={selectedTurnIndex}
          onSelectTurn={setSelectedTurnIndex}
          selectedFileIndex={selectedFileIndex}
          onSelectFile={setSelectedFileIndex}
          copiedKey={copiedKey}
          onCopy={copyText}
          spikeItems={spikeAnalysis}
        />
      ) : null}

      {/* 底部：跨会话搜索条 */}
      <SearchPanel
        searchQ={searchQ}
        onSearchQChange={setSearchQ}
        onSearch={() => void runSearch()}
        searching={searching}
        searchHits={searchHits}
      />
    </div>
  );
}
