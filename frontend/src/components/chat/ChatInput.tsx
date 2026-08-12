/**
 * 聊天输入区：自动增高文本域，框内下边缘工具条靠右（模型 ▾ 思考 ▾ + 发送），
 * 左侧留提示文字。Enter 发送 / Shift+Enter 换行。参照 ZCode 输入框设计。
 * 附件/常用语言/语音为占位按钮（hermes-webui 参考），功能后续接入。
 */
import { Fragment, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowUp, Brain, Languages, Mic, Paperclip, ShieldCheck } from "lucide-react";
import { api } from "@/api/client";
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
import { defaultModelValue, useAppStore } from "@/stores/app-store";

/**
 * 思考强度选项（对应 pi ThinkingLevel，label 用 chat.thinking.<value> 翻译）。
 * 默认四档为主流模型通用档位；具体档位按选中模型从后端动态获取
 * （/api/thinking-levels，复刻 pi 白名单），获取失败/加载中回退到默认四档。
 * minimal 不展示（UI 决策）；xhigh/max 仅白名单模型（gpt-5.6 系、
 * deepseek reasoning、claude opus 4.6+）会从后端返回。
 */
const DEFAULT_THINKING_VALUES = ["off", "low", "medium", "high"] as const;

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

/**
 * 当前模型的思考档位：模块级缓存（同一模型的档位请求全局只发一次，
 * 后端结果按模型固定，跨组件实例共享）；未加载/失败时回退默认四档。
 */
const thinkingLevelsCache = new Map<string, readonly string[]>();

function useThinkingLevels(modelValue: string | null): readonly string[] {
  const [, setVersion] = useState(0);
  useEffect(() => {
    if (!modelValue) return;
    if (thinkingLevelsCache.has(modelValue)) return;
    const [provider, model] = modelValue.split("::");
    if (!provider || !model) return;
    let cancelled = false;
    api
      .thinkingLevels(provider, model)
      .then(({ levels }) => {
        if (!cancelled && levels.length > 0) {
          thinkingLevelsCache.set(modelValue, levels);
          setVersion((v) => v + 1); // 通知本组件读取新缓存
        }
      })
      .catch(() => {
        /* 请求失败用默认四档兜底 */
      });
    return () => {
      cancelled = true;
    };
    // 缓存为模块级、setVersion 稳定：effect 仅在模型切换时触发，无空跑
  }, [modelValue]);
  return thinkingLevelsCache.get(modelValue ?? "") ?? DEFAULT_THINKING_VALUES;
}

export function ChatInput() {
  const { t } = useTranslation();
  const streaming = useAppStore((s) => s.streaming);
  const sendMessage = useAppStore((s) => s.sendMessage);
  const selectedModel = useAppStore((s) => s.selectedModel);
  const setSelectedModel = useAppStore((s) => s.setSelectedModel);
  const selectedThinking = useAppStore((s) => s.selectedThinking);
  const setSelectedThinking = useAppStore((s) => s.setSelectedThinking);
  const permissionMode = useAppStore((s) => s.permissionMode);
  const setPermissionMode = useAppStore((s) => s.setPermissionMode);
  const loadPermissionMode = useAppStore((s) => s.loadPermissionMode);
  const config = useAppStore((s) => s.config);

  // 挂载时读取插件权限模式（工具条展示用；设置页修改后刷新页面同步）
  useEffect(() => {
    void loadPermissionMode();
  }, [loadPermissionMode]);

  const modelGroups = useModelGroups();
  const [text, setText] = useState("");

  // 默认模型：全局默认 → 第一个提供商的默认模型（与 app-store 初始化逻辑同源）
  const defaultModel = useMemo(() => (config ? defaultModelValue(config) : null), [config]);

  const modelValue = selectedModel ?? defaultModel;
  // 当前选中模型的提供商 id（选择器前的小 logo 用）
  const modelProviderId = modelValue?.split("::")[0] ?? "";
  const canSend = text.trim().length > 0 && !streaming;
  // 当前模型的可用思考档位；已存档位不在列表时按最高可用档展示/发送
  // （与 pi 运行时 clamp 降级语义一致）
  const thinkingLevels = useThinkingLevels(modelValue);
  const effectiveThinking = thinkingLevels.includes(selectedThinking)
    ? selectedThinking
    : thinkingLevels[thinkingLevels.length - 1];

  const submit = async () => {
    if (!canSend) return;
    const value = text.trim();
    setText("");
    // provider 与 model 一起传：跨提供商切换模型时后端按请求级 provider 解析，
    // 避免 model 不属于会话原提供商导致路由降级（曾出现 401）
    const providerId = modelValue?.split("::")[0];
    const modelId = modelValue?.split("::")[1];
    await sendMessage(value, {
      provider: providerId ?? undefined,
      model: modelId ?? undefined,
      thinking: effectiveThinking,
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

              {/* 模型选择：提供商小 logo 在名称前。
                  value 传 null 而非 undefined：base-ui useControlled 在首渲染
                  锁定受控性（undefined → 非受控），默认模型是异步加载的，
                  传 undefined 会导致后续值变化被永久忽略（触发器永远占位符） */}
              <Select
                value={modelValue ?? null}
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
              {/* 思考强度选择（档位随选中模型动态变化） */}
              <Select
                value={effectiveThinking}
                onValueChange={setSelectedThinking}
                itemToStringLabel={(v) => t(`chat.thinking.${v}`)}
              >
                <SelectTrigger className="h-6 gap-1 border-transparent bg-transparent px-1.5 text-xs text-muted-foreground shadow-none hover:bg-accent hover:text-foreground data-[state=open]:bg-accent">
                  <Brain size={12} className="shrink-0 text-muted-foreground" />
                  <SelectValue />
                </SelectTrigger>
                <SelectContent align="start">
                  {thinkingLevels.map((value) => (
                    <SelectItem key={value} value={value} className="text-xs">
                      {t(`chat.thinking.${value}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>

              {/* 插件权限模式（安全/宽松/YOLO）：决定插件访问能力时是否需要询问。
                  模式变更即时保存到后端配置，新会话生效；YOLO 全自动放行（含危险能力） */}
              <Select
                value={permissionMode}
                onValueChange={(v) => {
                  if (v) void setPermissionMode(v);
                }}
                itemToStringLabel={(v) => t(`chat.permission.mode.${v}`)}
              >
                <SelectTrigger
                  className="h-6 gap-1 border-transparent bg-transparent px-1.5 text-xs text-muted-foreground shadow-none hover:bg-accent hover:text-foreground data-[state=open]:bg-accent"
                  title={t("chat.permission.modeTitle")}
                >
                  <ShieldCheck size={12} className="shrink-0 text-muted-foreground" />
                  <SelectValue />
                </SelectTrigger>
                <SelectContent align="start">
                  {(["default", "safe", "balanced", "yolo"] as const).map((value) => (
                    <SelectItem key={value} value={value} className="text-xs">
                      {t(`chat.permission.mode.${value}`)}
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
