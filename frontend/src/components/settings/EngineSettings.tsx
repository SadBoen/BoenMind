/**
 * 执行引擎设置：对话循环实现选择（自研 bm / 上游 pi / 跟随默认）。
 * 选择持久化到后端 config（loop_engine 字段）；未选择 = 跟随默认
 * （当前 pi，切换拍板后反转）。BM_LOOP_ENGINE 环境变量优先于本设置
 * （双开对比调试通道）。
 */
import { Bot, Cpu, Settings2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/app-store";
import { toast } from "sonner";

export function EngineSettings() {
  const { t } = useTranslation();
  const config = useAppStore((s) => s.config);
  const saveConfig = useAppStore((s) => s.saveConfig);

  const current = config?.loop_engine ?? null;

  const apply = async (key: string | null) => {
    if (!config) return;
    try {
      // null → 字段置 undefined：JSON 序列化丢弃，后端回落默认
      await saveConfig({ ...config, loop_engine: key ?? undefined });
      toast.success(t("settings.engine.saved"));
    } catch (err) {
      toast.error(t("settings.engine.saveFailed", { error: String(err) }));
    }
  };

  const OPTIONS = [
    {
      key: null,
      icon: <Settings2 size={16} />,
      labelKey: "settings.engine.follow",
      descKey: "settings.engine.followDesc",
    },
    {
      key: "bm",
      icon: <Cpu size={16} />,
      labelKey: "settings.engine.bm",
      descKey: "settings.engine.bmDesc",
    },
    {
      key: "pi",
      icon: <Bot size={16} />,
      labelKey: "settings.engine.pi",
      descKey: "settings.engine.piDesc",
    },
  ] as const;

  return (
    <section className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">{t("settings.engine.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("settings.engine.desc")}</p>
      </div>

      <div className="grid grid-cols-3 gap-3">
        {OPTIONS.map((opt) => {
          const selected = current === opt.key;
          return (
            <button
              key={opt.key ?? "default"}
              type="button"
              onClick={() => void apply(opt.key)}
              className={`flex flex-col items-start gap-2 rounded-xl border-2 p-4 text-left transition-colors ${
                selected
                  ? "border-primary bg-primary/5"
                  : "border-border hover:border-muted-foreground/40"
              }`}
            >
              {opt.icon}
              <span className="text-sm font-medium">{t(opt.labelKey)}</span>
              <span className="text-xs text-muted-foreground">{t(opt.descKey)}</span>
            </button>
          );
        })}
      </div>

      <p className="text-xs text-muted-foreground">{t("settings.engine.hint")}</p>
    </section>
  );
}
