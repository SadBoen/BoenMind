import { useEffect, useRef } from "react";
import { formatBytes, uid } from "../lib/format";
import { IconPaperclip, IconSend, IconStar, IconStop } from "../lib/icons";
import { useChatActions, useStore } from "../store";
import { toast } from "../lib/toast";
import type { ProviderConfig } from "../types";

/** 模型下拉选项：本地配置的提供商模型（带提供商名）+ 后端 llm.models 装配模型。 */
function composerModelOptions(
  providers: ProviderConfig[],
  backend: { provider: string; models: string[] }[],
): { value: string; label: string }[] {
  const out: { value: string; label: string }[] = [];
  const seen = new Set<string>();
  const push = (value: string, label: string) => {
    if (!value || seen.has(value)) return;
    seen.add(value);
    out.push({ value, label });
  };
  for (const p of providers) {
    for (const m of p.models) push(m, `${p.name || p.kind} · ${m}`);
  }
  for (const g of backend) {
    for (const m of g.models) push(m, `${g.provider} · ${m}`);
  }
  return out;
}

export function Composer() {
  const { state, dispatch } = useStore();
  const send = useChatActions();
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    const max = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--font-body")) * 1.5 * 8;
    el.style.height = `${Math.min(el.scrollHeight, max || 192)}px`;
  }, [state.composer]);

  const empty = !state.composer.trim() && state.composerAttachments.length === 0;
  const baseOptions = composerModelOptions(state.settings.providers, state.backendModels);
  // 当前选中不在选项里（如后端模型目录变化）→ 原样补进选项保持显示与发送一致
  //（回归：曾静默回落到第一项显示，实际仍按旧 model 发送）。
  const modelOptions =
    state.model && !baseOptions.some((o) => o.value === state.model)
      ? [{ value: state.model, label: state.model }, ...baseOptions]
      : baseOptions;
  const selectedModel = modelOptions.some((o) => o.value === state.model) ? state.model : (modelOptions[0]?.value ?? "");

  return (
    <div className="composer-wrap">
      <div className={`composer${state.streaming ? " is-live" : ""}`}>
        {state.composerAttachments.map((a) => (
          <span key={a.id} className="attach-chip">
            {a.name} · {formatBytes(a.size)}
            <button type="button" className="icon-btn" aria-label="移除附件" onClick={() => dispatch({ type: "remove-attachment", id: a.id })}>
              ×
            </button>
          </span>
        ))}
        <textarea
          ref={ref}
          className="composer-text"
          rows={2}
          placeholder="写一条消息…"
          value={state.composer}
          onChange={(e) => dispatch({ type: "set-composer", value: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
        />
        <div className="composer-foot">
          <button
            type="button"
            className="icon-btn"
            aria-label="附件"
            title="附件"
            onClick={() => {
              const input = document.createElement("input");
              input.type = "file";
              input.onchange = () => {
                const f = input.files?.[0];
                if (!f) return;
                dispatch({
                  type: "add-attachment",
                  file: { id: uid("a"), name: f.name, size: f.size, type: f.type || "application/octet-stream" },
                });
              };
              input.click();
            }}
          >
            <IconPaperclip />
          </button>
          <button type="button" className="icon-btn" aria-label="收藏" title="收藏" onClick={() => toast.info("收藏占位")}>
            <IconStar />
          </button>
          <div className="composer-right">
            <select className="field" value={selectedModel} onChange={(e) => dispatch({ type: "set-model", model: e.target.value })} aria-label="模型">
              {modelOptions.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
              {modelOptions.length === 0 && <option value="">未配置</option>}
            </select>
            <select
              className="field"
              value={state.reasoning}
              onChange={(e) => dispatch({ type: "set-reasoning", reasoning: e.target.value as typeof state.reasoning })}
              aria-label="推理"
            >
              <option value="off">off</option>
              <option value="low">low</option>
              <option value="medium">medium</option>
              <option value="high">high</option>
            </select>
            <button
              type="button"
              className={`send-btn${state.streaming ? " is-stop" : ""}`}
              disabled={!state.streaming && empty}
              aria-label={state.streaming ? "Stop" : "发送"}
              onClick={send}
            >
              {state.streaming ? <IconStop /> : <IconSend />}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
