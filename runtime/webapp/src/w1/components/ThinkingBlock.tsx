import { useState, useRef, useEffect, UIEvent } from "react";
import { Brain, ChevronDown, ChevronRight, Maximize2, Minimize2 } from "lucide-react";

interface ThinkingBlockProps {
  text: string;
  isStreaming?: boolean;
  elapsedSeconds?: number;
}

/**
 * 思考块组件:
 * 1. 脑子小图标 + 状态说明（思考 · 持续了几秒 / 正在思考）
 * 2. 默认折叠，点击展开
 * 3. 展开限高 3 行左右（约 4.5rem），使用 CSS Mask 渐变羽化遮罩代替传统滚动条
 * 4. 支持滚轮平滑滑动查看，并提供一键全部展开/还原小按钮
 */
export function ThinkingBlock({ text, isStreaming = false, elapsedSeconds }: ThinkingBlockProps) {
  const [open, setOpen] = useState(false);
  const [fullyExpanded, setFullyExpanded] = useState(false);
  const [scrollMask, setScrollMask] = useState<"both" | "top" | "bottom" | "none">("none");
  const scrollContainerRef = useRef<HTMLDivElement>(null);

  // 动态计算顶部/底部渐隐遮罩状态 (借鉴 ZCode TD=24 遮罩算法)
  const updateMask = () => {
    const el = scrollContainerRef.current;
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
  }, [open, text, fullyExpanded]);

  // 生成顶部与底部 20px 渐变遮罩样式
  const getMaskStyle = () => {
    if (fullyExpanded) return undefined;
    const TD = 20;
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

  // 整理思考文案
  const getSummaryText = () => {
    if (isStreaming) {
      return "正在思考…";
    }
    if (elapsedSeconds && elapsedSeconds > 0) {
      return `思考 · 持续了 ${elapsedSeconds.toFixed(1)} 秒`;
    }
    return "思考 · 持续了几秒";
  };

  return (
    <div className="my-1.5 rounded-lg border border-border/40 bg-card/60 overflow-hidden transition-all duration-200">
      {/* 头部点击折叠/展开 */}
      <div
        className="flex items-center gap-2 px-2.5 py-1.5 cursor-pointer select-none text-xs text-muted-foreground hover:bg-muted/40 transition-colors"
        onClick={() => setOpen(!open)}
      >
        {open ? <ChevronDown size={13} className="shrink-0" /> : <ChevronRight size={13} className="shrink-0" />}
        <Brain className={`size-3.5 shrink-0 ${isStreaming ? "text-purple-400 animate-pulse" : "text-purple-500/80"}`} />
        <span className={`font-medium flex-1 ${isStreaming ? "animated-gradient-text" : "text-foreground/80"}`}>
          {getSummaryText()}
        </span>
        <span className="text-[11px] text-muted-foreground/60">{open ? "收起" : "展开"}</span>
      </div>

      {/* 展开内容 */}
      {open ? (
        <div className="border-t border-dashed border-border/60 bg-muted/20 px-3 py-2">
          <div className="flex items-center justify-between pb-1 text-[11px] text-muted-foreground/70">
            <span>思考过程内容（可鼠标滚轮平滑预览）</span>
            <button
              className="inline-flex items-center gap-1 hover:text-foreground transition-colors"
              onClick={(e) => {
                e.stopPropagation();
                setFullyExpanded(!fullyExpanded);
              }}
              title={fullyExpanded ? "恢复限高" : "展开全部"}
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
            ref={scrollContainerRef}
            onScroll={handleScroll}
            style={getMaskStyle()}
            className={`text-xs font-mono leading-relaxed text-foreground/85 whitespace-pre-wrap break-words overflow-y-auto no-scrollbar transition-all duration-200 pl-2 border-l-2 border-purple-500/30 ${
              fullyExpanded ? "max-h-96" : "max-h-[4.5rem]"
            }`}
          >
            {text}
          </div>
        </div>
      ) : null}
    </div>
  );
}
