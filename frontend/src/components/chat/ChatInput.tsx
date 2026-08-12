/**
 * 聊天输入区：自动增高文本域，框内下边缘工具条靠右（模型 ▾ 思考 ▾ + 发送），
 * 左侧留提示文字。Enter 发送 / Shift+Enter 换行。参照 ZCode 输入框设计。
 * 附件/常用语言/语音为占位按钮（hermes-webui 参考），功能后续接入。
 */
import { Fragment, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowUp, Brain, Languages, Mic, Paperclip } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ProviderIcon } from "@/components/settings/provider-icons";
import { useAppStore } from "@/stores/app-store";

/**
 * 思考强度选项（对应 pi ThinkingLevel，label 用 chat.thinking.<value> 翻译）。
 * 只保留主流模型通用的 off/low/medium/high 四档：xhigh/max 是少数新旗舰
 * （GPT-5.2+/5.6、DeepSeek reasoning、Claude max）专属，多数模型会被 pi
 * 按模型能力 clamp 降级，UI 不展示。
 */
const THINKING_VALUES = ["off", "low", "medium", "high"] as const;

interface ModelGroup {
  id: string;
  name: string;
  models: { value: string; label: string }[];
}

/** 模型选项按提供商分组（providerId::modelId），组间以分割线区分 */
function useModelGroups(): ModelGroup[] {
  const config = useAppStore((s) => s.config);
  return useMemo(() => {
    if (!config) return [];
    return config.providers.map((p) => ({
      id: p.id,
      name: p.name,
      models: p.models.map((m) => ({
        value: `${p.id}::${m}`,
        label: m,
      })),
    }));
  }, [config]);
}

export function ChatInput() {
  const { t } = useTranslation();
  const streaming = useAppStore((s) => s.streaming);
  const sendMessage = useAppStore((s) => s.sendMessage);
  const selectedModel = useAppStore((s) => s.selectedModel);
  const setSelectedModel = useAppStore((s) => s.setSelectedModel);
  const selectedThinking = useAppStore((s) => s.selectedThinking);
  const setSelectedThinking = useAppStore((s) => s.setSelectedThinking);
  const config = useAppStore((s) => s.config);

  const modelGroups = useModelGroups();
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
  // 当前选中模型的提供商 id（选择器前的小 logo 用）
  const modelProviderId = modelValue?.split("::")[0] ?? "";
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
      <div className="px-1">
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
            placeholder={t("chat.input.placeholder")}
            rows={1}
            className="max-h-40 min-h-[2.25rem] resize-none border-0 bg-transparent p-3 pb-1 text-sm shadow-none focus-visible:ring-0"
            disabled={streaming}
          />
          {/* 框内下边缘工具条：提示在左，模型/思考/发送整体靠右 */}
          <div className="flex items-center justify-between gap-2 px-2 pb-1.5">
            <span className="hidden shrink-0 text-[10px] text-muted-foreground sm:inline">
              {text.length > 0
                ? t("chat.input.charCount", { count: text.length })
                : t("chat.input.enterToSend")}
            </span>
            <div className="flex min-w-0 items-center gap-1">
              {/* 占位按钮（hermes-webui 参考，功能后续接入） */}
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 rounded-md text-muted-foreground"
                disabled
                title={t("chat.input.attach")}
              >
                <Paperclip size={13} />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 rounded-md text-muted-foreground"
                disabled
                title={t("chat.input.language")}
              >
                <Languages size={13} />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 rounded-md text-muted-foreground"
                disabled
                title={t("chat.input.voice")}
              >
                <Mic size={13} />
              </Button>

              {/* 模型选择：提供商小 logo 在名称前 */}
              <Select
                value={modelValue ?? undefined}
                onValueChange={setSelectedModel}
                itemToStringLabel={(v) => String(v).split("::")[1] ?? ""}
              >
                <SelectTrigger className="h-6 max-w-56 gap-1 border-transparent bg-transparent px-1.5 text-xs text-muted-foreground shadow-none hover:bg-accent hover:text-foreground data-[state=open]:bg-accent">
                  <ProviderIcon kind={modelProviderId} size={12} />
                  <SelectValue placeholder={t("chat.input.selectModel")} />
                </SelectTrigger>
                <SelectContent align="start">
                  {modelGroups.length === 0 && (
                    <p className="px-2 py-1.5 text-xs text-muted-foreground">
                      {t("chat.input.noProviders")}
                    </p>
                  )}
                  {modelGroups.map((group, gi) => (
                    <Fragment key={group.id}>
                      {gi > 0 && <SelectSeparator />}
                      <SelectGroup>
                        <SelectLabel className="flex items-center gap-1.5">
                          <ProviderIcon kind={group.id} size={11} label={group.name} />
                          {group.name}
                        </SelectLabel>
                        {group.models.map((opt) => (
                          <SelectItem key={opt.value} value={opt.value} label={opt.label} className="text-xs">
                            {opt.label}
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    </Fragment>
                  ))}
                </SelectContent>
              </Select>
              {/* 思考强度选择 */}
              <Select
                value={selectedThinking}
                onValueChange={setSelectedThinking}
                itemToStringLabel={(v) => t(`chat.thinking.${v}`)}
              >
                <SelectTrigger className="h-6 gap-1 border-transparent bg-transparent px-1.5 text-xs text-muted-foreground shadow-none hover:bg-accent hover:text-foreground data-[state=open]:bg-accent">
                  <Brain size={12} className="shrink-0 text-muted-foreground" />
                  <SelectValue />
                </SelectTrigger>
                <SelectContent align="start">
                  {THINKING_VALUES.map((value) => (
                    <SelectItem key={value} value={value} className="text-xs">
                      {t(`chat.thinking.${value}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>

              <Button
                size="icon"
                className="ml-0.5 h-7 w-7 rounded-lg"
                onClick={() => void submit()}
                disabled={!canSend}
                title={t("chat.input.send")}
              >
                <ArrowUp size={15} />
              </Button>
            </div>
          </div>
        </div>
        <p className="mt-1.5 text-center text-[10px] text-muted-foreground">
          {t("chat.input.aiDisclaimer")}
        </p>
      </div>
    </div>
  );
}
