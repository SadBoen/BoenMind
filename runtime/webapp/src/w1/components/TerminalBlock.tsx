import { useState, useRef, useEffect, UIEvent } from "react";
import { Terminal, ChevronDown, ChevronRight, Maximize2, Minimize2, CheckCircle2 } from "lucide-react";
import { ToolActionItem } from "../parser";

interface TerminalBlockProps {
  item: ToolActionItem;
  isRunning?: boolean;
}

/**
 * 终端命令执行卡片组件:
 * 1. 终端方形图标 (SquareTerminal / Terminal)
 * 2. 默认折叠展示命令单行，点击可展开日志
 * 3. 限高 2~3 行，支持滚动查看且隐藏原生滚动条，采用渐变羽化遮罩
 */
export function TerminalBlock({ item, isRunning: _isRunning = false }: TerminalBlockProps) {
  const [open, setOpen] = useState(false);
  const [fullyExpanded, setFullyExpanded] = useState(false);
  const [scrollMask, setScrollMask] = useState<"both" | "top" | "bottom" | "none">("none");
  const scrollRef = useRef<HTMLDivElement>(null);

  const updateMask = () => {
    const el = scrollRef.current;
    if (!el) return;
    const { scrollTop, scrollHeight, clientHeight } = el;
    const maxScroll = Math.max(0, scrollHeight - clientHeight);
    if (maxScroll <= 2) {
      setScrollMask("none");
      return;
    }
    const showTop = scrollTop > 2;
    const showBottom = scrollTop < maxScroll - 2;
    if (showTop && showBottom) setScrollMask("both");
    else if (showTop) setScrollMask("top");
    else if (showBottom) setScrollMask("bottom");
    else setScrollMask("none");
  };

  const handleScroll = (_e: UIEvent<HTMLDivElement>) => {
    updateMask();
  };

  useEffect(() => {
    if (open) {
      updateMask();
    }
  }, [open, item.target, fullyExpanded]);

  const getMaskStyle = () => {
    if (fullyExpanded) return undefined;
    const TD = 18;
    if (scrollMask === "none") return undefined;

    const showTop = scrollMask === "both" || scrollMask === "top";
    const showBottom = scrollMask === "both" || scrollMask === "bottom";

    const gradient = `linear-gradient(to bottom, ${
      showTop ? `transparent 0px, black ${TD}px` : `black 0px, black ${TD}px`
    }, ${
      showBottom ? `black calc(100% - ${TD}px), transparent 100%` : `black calc(100% - ${TD}px), black 100%`
    })`;

    return {
      WebkitMaskImage: gradient,
      maskImage: gradient,
      WebkitMaskRepeat: "no-repeat",
      maskRepeat: "no-repeat",
      WebkitMaskSize: "100% 100%",
      maskSize: "100% 100%",
    };
  };

  const command = item.target || item.name;

  return (
    <div className="rounded-lg border border-border/40 bg-card/60 overflow-hidden transition-all duration-200">
      {/* 头部单行 */}
      <div
        className="flex items-center gap-2 px-2.5 py-1 cursor-pointer select-none text-xs hover:bg-muted/40 transition-colors"
        onClick={() => setOpen(!open)}
      >
        {open ? <ChevronDown size={13} className="shrink-0 text-muted-foreground" /> : <ChevronRight size={13} className="shrink-0 text-muted-foreground" />}
        <Terminal className="size-3.5 shrink-0 text-emerald-500" />
        <span className="font-semibold text-foreground/90 shrink-0">终端</span>
        <span className="font-mono text-[11px] text-muted-foreground truncate flex-1 bg-muted/40 px-1.5 py-0.5 rounded border border-border/30">
          {command}
        </span>
        {item.elapsedMs ? (
          <span className="text-[10.5px] font-mono text-emerald-500/90 shrink-0">
            {item.elapsedMs}ms
          </span>
        ) : null}
        <span className="text-[11px] text-muted-foreground/60 shrink-0">
          {open ? "收起" : "展开"}
        </span>
      </div>

      {/* 展开的命令详情与输出 */}
      {open ? (
        <div className="border-t border-dashed border-border/60 bg-muted/20 px-3 py-2">
          <div className="flex items-center justify-between pb-1 text-[11px] text-muted-foreground/70">
            <span className="flex items-center gap-1">
              <CheckCircle2 size={11} className="text-emerald-500" /> 已就绪 / 可滚轮查看命令
            </span>
            <button
              className="inline-flex items-center gap-1 hover:text-foreground transition-colors"
              onClick={(e) => {
                e.stopPropagation();
                setFullyExpanded(!fullyExpanded);
              }}
            >
              {fullyExpanded ? (
                <>
                  <Minimize2 size={11} /> 恢复限高
                </>
              ) : (
                <>
                  <Maximize2 size={11} /> 展开全部
                </>
              )}
            </button>
          </div>

          <div
            ref={scrollRef}
            onScroll={handleScroll}
            style={getMaskStyle()}
            className={`font-mono text-[11.5px] leading-relaxed text-foreground/90 bg-neutral-900/90 text-neutral-100 dark:bg-black/60 rounded p-2 overflow-y-auto no-scrollbar transition-all duration-200 ${
              fullyExpanded ? "max-h-80" : "max-h-[4.5rem]"
            }`}
          >
            <div className="text-emerald-400 select-none pb-0.5">$ {command}</div>
            <div className="text-muted-foreground text-[11px]">... 命令已发送给环境执行 ...</div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
