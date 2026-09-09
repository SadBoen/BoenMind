//! 底部跨会话检索面板(#22 拆分:自 context.tsx 机械移入)
//! 在历史所有问答中搜索上下文或工具结果片段。

import { Button } from "@/components/ui/button";
import type { CtxStep } from "@/w2/api";

export function SearchPanel({
  searchQ,
  onSearchQChange,
  onSearch,
  searching,
  searchHits,
}: {
  searchQ: string;
  onSearchQChange: (q: string) => void;
  onSearch: () => void;
  searching: boolean;
  searchHits: CtxStep[] | null;
}) {
  return (
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
          placeholder="输入关键词搜索（如：天气 / read / 某个报错）"
          value={searchQ}
          onChange={(e) => onSearchQChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") onSearch();
          }}
        />
        <Button
          size="sm"
          className="h-8 px-3 text-[12px]"
          disabled={searching || !searchQ.trim()}
          onClick={onSearch}
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
  );
}
