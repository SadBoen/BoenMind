/**
 * 聊天输入区：自动增高文本域，框内下边缘工具条（模型 ▾ 思考 ▾ + 发送）。
 * Enter 发送 / Shift+Enter 换行。参照 ZCode 输入框设计。
 */
import { useMemo, useState } from "react";
import { ArrowUp, Brain, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useAppStore } from "@/stores/app-store";

/** 思考强度选项（对应 pi ThinkingLevel） */
const THINKING_OPTIONS = [
  { value: "off", label: "不思考" },
  { value: "minimal", label: "极低" },
  { value: "low", label: "低" },
  { value: "medium", label: "中" },
  { value: "high", label: "高" },
  { value: "xhigh", label: "极高" },
  { value: "max", label: "最大" },
];

/** 当前可用的模型选项：各提供商的模型（providerId::modelId） */
function useModelOptions() {
  const config = useAppStore((s) => s.config);
  return useMemo(() => {
    if (!config) return [];
    return config.providers.flatMap((p) =>
      p.models.map((m) => ({
        value: `${p.id}::${m}`,
        label: m,
        provider: p.name,
      })),
    );
  }, [config]);
}

export function ChatInput() {
  const streaming = useAppStore((s) => s.streaming);
  const sendMessage = useAppStore((s) => s.sendMessage);
  const selectedModel = useAppStore((s) => s.selectedModel);
  const setSelectedModel = useAppStore((s) => s.setSelectedModel);
  const selectedThinking = useAppStore((s) => s.selectedThinking);
  const setSelectedThinking = useAppStore((s) => s.setSelectedThinking);
  const config = useAppStore((s) => s.config);

  const modelOptions = useModelOptions();
  const [text, setText] = useState("");

  // 默认模型：全局默认 → 第一个提供商的默认模型
  const defaultModel = useMemo(() => {
    if (!config) return null;
    const def = config.default_model;
    if (def) {
      const p = config.providers.find((p) => p.default_model === def || p.models.includes(def));
      if (p) return `${p.id}::${def}`;
    }
    const p = config.providers[0];
    if (p?.models[0]) return `${p.id}::${p.models[0]}`;
    return null;
  }, [config]);

  const modelValue = selectedModel ?? defaultModel;
  const canSend = text.trim().length > 0 && !streaming;

  const submit = async () => {
    if (!canSend) return;
    const value = text.trim();
    setText("");
    const modelId = modelValue?.split("::")[1];
    await sendMessage(value, {
      model: modelId ?? undefined,
      thinking: selectedThinking,
    });
  };

  return (
    <div className="shrink-0 border-t p-3">
      <div className="mx-auto max-w-3xl">
        {/* 输入框（含内部下边缘工具条） */}
        <div className="rounded-xl border bg-background shadow-sm transition-shadow focus-within:shadow-md focus-within:ring-1 focus-within:ring-ring">
          <Textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                void submit();
              }
            }}
            placeholder="输入消息，Enter 发送，Shift+Enter 换行"
            rows={1}
            className="max-h-40 min-h-[2.25rem] resize-none border-0 bg-transparent p-3 pb-1 text-sm shadow-none focus-visible:ring-0"
            disabled={streaming}
          />
          {/* 框内下边缘工具条 */}
          <div className="flex items-center justify-between gap-2 px-2 pb-1.5">
            <div className="flex min-w-0 items-center gap-1.5">
              {/* 模型选择 */}
              <Select value={modelValue ?? undefined} onValueChange={setSelectedModel}>
                <SelectTrigger className="h-6 max-w-40 gap-1 border-transparent bg-transparent px-1.5 text-xs text-muted-foreground shadow-none hover:bg-accent hover:text-foreground data-[state=open]:bg-accent">
                  <SelectValue placeholder="选择模型" />
                  <Sparkles size={12} className="shrink-0 text-muted-foreground" />
                </SelectTrigger>
                <SelectContent align="start">
                  {modelOptions.length === 0 && (
                    <p className="px-2 py-1.5 text-xs text-muted-foreground">
                      未配置提供商，请到设置中添加
                    </p>
                  )}
                  {modelOptions.map((opt) => (
                    <SelectItem key={opt.value} value={opt.value} className="text-xs">
                      {opt.label}
                      <span className="ml-1.5 text-[10px] text-muted-foreground">{opt.provider}</span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {/* 思考强度选择 */}
              <Select value={selectedThinking} onValueChange={setSelectedThinking}>
                <SelectTrigger className="h-6 gap-1 border-transparent bg-transparent px-1.5 text-xs text-muted-foreground shadow-none hover:bg-accent hover:text-foreground data-[state=open]:bg-accent">
                  <SelectValue />
                  <Brain size={12} className="shrink-0 text-muted-foreground" />
                </SelectTrigger>
                <SelectContent align="start">
                  {THINKING_OPTIONS.map((opt) => (
                    <SelectItem key={opt.value} value={opt.value} className="text-xs">
                      {opt.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="flex items-center gap-2">
              <span className="hidden text-[10px] text-muted-foreground sm:inline">
                {text.length > 0 ? `${text.length} 字` : "Enter 发送"}
              </span>
              <Button
                size="icon"
                className="h-7 w-7 rounded-lg"
                onClick={() => void submit()}
                disabled={!canSend}
                title="发送"
              >
                <ArrowUp size={15} />
              </Button>
            </div>
          </div>
        </div>
        <p className="mt-1.5 text-center text-[10px] text-muted-foreground">
          内容由 AI 生成，请核实重要信息
        </p>
      </div>
    </div>
  );
}
