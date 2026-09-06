import { useEffect, useState, useRef } from "react";
import { Loader2, Sparkles, Brain, Terminal, Search, FileEdit } from "lucide-react";

interface AgentStatusBarProps {
  isRunning: boolean;
  currentAction?: string;
  activeModel?: string;
}

/**
 * 吸底动态处理状态条:
 * 1. 位于输入框正上方（composer-dock 内），固定不随消息向上滚动而沉降
 * 2. 左侧为转动中的 Loader2 / Brain 动态微光图标，表示正在工作
 * 3. 右侧包含流光渐变文本（animated-gradient-text）+ 垂直轮播跑马灯（Ticker），展示正在进行的动作
 * 4. 包含运行时已耗时实时计时器（如 3.2s）
 * 5. 任务结束时自动淡出与收起
 */
export function AgentStatusBar({ isRunning, currentAction, activeModel }: AgentStatusBarProps) {
  const [seconds, setSeconds] = useState(0);
  const [tickerIndex, setTickerIndex] = useState(0);
  const timerRef = useRef<number | null>(null);

  // 轮播候选项列表：若模型没有具体输出当前子步骤，提供平滑循环动作提示
  const defaultActionList = [
    "正在分析上下文与用户意图…",
    "检索相关代码与工程符号…",
    "规划操作步骤与安全边界…",
    "实时组织输出结构…",
  ];

  // 耗时计时器
  useEffect(() => {
    if (isRunning) {
      setSeconds(0);
      const start = Date.now();
      timerRef.current = window.setInterval(() => {
        setSeconds((Date.now() - start) / 1000);
      }, 100);
    } else {
      if (timerRef.current) window.clearInterval(timerRef.current);
      setSeconds(0);
    }
    return () => {
      if (timerRef.current) window.clearInterval(timerRef.current);
    };
  }, [isRunning]);

  // 垂直轮播状态跑马灯（每 2.2 秒翻页一次）
  useEffect(() => {
    if (!isRunning) return;
    const tickerTimer = setInterval(() => {
      setTickerIndex((prev) => (prev + 1) % defaultActionList.length);
    }, 2200);
    return () => clearInterval(tickerTimer);
  }, [isRunning, defaultActionList.length]);

  if (!isRunning) return null;

  const displayAction = currentAction || defaultActionList[tickerIndex];

  // 根据当前动作匹配图标
  const getActionIcon = () => {
    const lower = displayAction.toLowerCase();
    if (lower.includes("思考") || lower.includes("think") || lower.includes("分析")) {
      return <Brain className="size-3.5 text-purple-400 shrink-0 animate-pulse" />;
    }
    if (lower.includes("终端") || lower.includes("exec") || lower.includes("bash")) {
      return <Terminal className="size-3.5 text-rose-500 shrink-0" />;
    }
    if (lower.includes("读") || lower.includes("查") || lower.includes("search") || lower.includes("检索")) {
      return <Search className="size-3.5 text-sky-400 shrink-0" />;
    }
    if (lower.includes("改") || lower.includes("写") || lower.includes("edit")) {
      return <FileEdit className="size-3.5 text-emerald-400 shrink-0" />;
    }
    return <Sparkles className="size-3.5 text-amber-400 shrink-0" />;
  };

  return (
    <div className="mb-2 w-full animate-in fade-in slide-in-from-bottom-2 duration-200">
      <div className="flex items-center justify-between gap-2.5 px-3 py-1.5 rounded-lg border border-border/50 bg-card/85 backdrop-blur-md shadow-sm text-xs select-none">
        {/* 左侧动态 Spinner 与状态指示 */}
        <div className="flex items-center gap-2 min-w-0 flex-1">
          <Loader2 className="size-3.5 text-primary shrink-0 animate-spin" />
          {getActionIcon()}

          {/* 垂直滑动 Ticker 容器 */}
          <div className="relative h-4.5 overflow-hidden flex-1 min-w-0">
            <span
              key={displayAction}
              className="inline-block truncate font-medium text-foreground/90 animated-gradient-text transition-all duration-300"
            >
              {displayAction}
            </span>
          </div>
        </div>

        {/* 右侧耗时计时与模型小标 */}
        <div className="flex items-center gap-2 shrink-0 font-mono text-[11px] text-muted-foreground/80">
          {activeModel ? (
            <span className="hidden sm:inline-block px-1.5 py-0.2 rounded bg-muted/40 text-[10px]">
              {activeModel}
            </span>
          ) : null}
          <span className="tabular-nums font-medium text-foreground/75">
            {seconds.toFixed(1)}s
          </span>
        </div>
      </div>
    </div>
  );
}
