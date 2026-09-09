//! 上下文页视图块共享类型(#22 拆分:context.tsx 派生结果的形状契约)。
//! 各视图块组件(HealthBoard/TrendChart/tabs/SearchPanel)经 props 接收
//! 这些派生值,派生逻辑仍集中在 context.tsx 装配层。

/** token 篇幅/速率/水位统计(stats memo 的输出形状) */
export type ContextStats = {
  personaTokens: number;
  skillsTokens: number;
  wsTokens: number;
  toolsTokens: number;
  historyTokens: number;
  inputTokens: number;
  totalEst: number;
  realTokensIn: number;
  realTokensOut: number;
  maxWindow: number | null;
  remainingHeadroom: number | null;
  headroomPct: number | null;
  speed: string;
  reasoningTokens: number | null;
  reasoningSnippetEstimated: number | null;
  cachedTokens: number | null;
  ttftMs: number | null;
  evictedTurns: number;
  pct: {
    persona: number;
    skills: number;
    ws: number;
    tools: number;
    history: number;
    input: number;
  };
};

/** DSH 时序趋势序列单条(按单步或按轮次聚合) */
export type TrendItem = {
  id: string;
  label: string;
  turn_index: number;
  step: number;
  tokens_in: number;
  tokens_out: number;
  evicted_turns: number;
  pTok: number;
  skTok: number;
  wsTok: number;
  toolTok: number;
  histTok: number;
  inTok: number;
  total: number;
  question: string;
  lastInput: string;
  responseSummary: string;
};

/** Delta 增量模式单条(相对上一步的 token 差值) */
export type DeltaTrendItem = {
  id: string;
  label: string;
  delta: number;
  isNegative: boolean;
  magnitude: number;
  tokens_in: number;
  tokens_out: number;
};

/** 多轮历史 Token 暴增刺客诊断单条 */
export type SpikeItem = {
  seq: number;
  turn_index: number;
  step: number;
  model_id: string;
  tokens_in: number;
  tokens_out: number;
  diff: number;
  isSpike: boolean;
};
