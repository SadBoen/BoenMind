/**
 * 每软件 APP 设置页（设置架构 §五）：专家绑定 / 记忆 / 工作区 + 独有扩展视图。
 * 所有带 LLM 的 APP（chat/coding/…）共用此模板，appId 由注册表传入。
 * 底层 LLM 交互引擎所有 APP 共用一套（单源 config.toml 的 [apps.<id>] 段）。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Puzzle, Wand2 } from "lucide-react";
import { toast } from "sonner";
import { api, type AppProfile, type ExpertDef } from "@/api/client";
import { useAppStore } from "@/stores/app-store";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Badge } from "@/components/ui/badge";

export function AppSettings({ appId }: { appId: string }) {
  const { t } = useTranslation();
  const config = useAppStore((s) => s.config);
  const loadConfig = useAppStore((s) => s.loadConfig);
  const [experts, setExperts] = useState<ExpertDef[]>([]);
  const [expert, setExpert] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setExpert(config?.apps?.[appId]?.expert ?? null);
  }, [config, appId]);

  useEffect(() => {
    api
      .listExperts()
      .then(setExperts)
      .catch(() => setExperts([]));
  }, []);

  const save = async (profile: AppProfile) => {
    setSaving(true);
    try {
      await api.putAppProfile(appId, profile);
      await loadConfig();
      toast.success(t("settings.appSettings.saved", { app: t(`settings.menu.app${appId === "chat" ? "Chat" : "Coding"}`) }));
    } catch (err) {
      toast.error(t("settings.appSettings.saveFailed", { error: String(err) }));
    } finally {
      setSaving(false);
    }
  };

  const bindExpert = (value: string | null) => {
    setExpert(value);
    void save({ expert: value || undefined });
  };

  // 该 APP 独有扩展（作用域 = 本 APP 的插件与 skill）
  const exclusivePlugins = Object.entries(config?.pluginScopes ?? {}).filter(([, v]) => v.includes(appId));
  const exclusiveSkills = Object.entries(config?.skillScopes ?? {}).filter(([, v]) => v.includes(appId));
  const profile = config?.apps?.[appId];
  const appName = t(`settings.menu.app${appId === "chat" ? "Chat" : "Coding"}`);

  return (
    <section className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">{appName}</h2>
        <p className="text-sm text-muted-foreground">{t("settings.appSettings.desc")}</p>
      </div>

      {/* 模型与专家：该 APP 绑定管家派的专家 */}
      <div className="rounded-xl border p-4">
        <h3 className="text-sm font-semibold">{t("settings.appSettings.expertTitle")}</h3>
        <p className="text-xs text-muted-foreground">{t("settings.appSettings.expertDesc")}</p>
        <div className="mt-3 flex items-center gap-2">
          <Select
            value={expert ?? ""}
            onValueChange={bindExpert}
            disabled={saving}
          >
            <SelectTrigger className="w-64">
              <SelectValue placeholder={t("settings.appSettings.expertNone")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="">{t("settings.appSettings.expertNone")}</SelectItem>
              {experts.map((e) => (
                <SelectItem key={e.name} value={e.name}>
                  {e.name} — {e.description.slice(0, 40)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {saving && <Loader2 size={14} className="animate-spin text-muted-foreground" />}
        </div>
        <p className="mt-2 text-xs text-muted-foreground">{t("settings.appSettings.expertHint")}</p>
      </div>

      {/* 扩展：该 APP 独有（公共扩展对所有 APP 生效） */}
      <div className="rounded-xl border p-4">
        <h3 className="flex items-center gap-1.5 text-sm font-semibold">
          <Puzzle size={14} className="text-muted-foreground" />
          {t("settings.appSettings.extTitle")}
        </h3>
        <p className="text-xs text-muted-foreground">{t("settings.appSettings.extDesc")}</p>
        <div className="mt-3 flex flex-wrap gap-1.5">
          {exclusivePlugins.map(([id]) => (
            <Badge key={`p:${id}`} variant="outline" className="text-[10px] font-normal">
              {id}
            </Badge>
          ))}
          {exclusiveSkills.map(([id]) => (
            <Badge key={`s:${id}`} variant="outline" className="text-[10px] font-normal">
              <Wand2 size={10} className="mr-1" />
              {id}
            </Badge>
          ))}
          {exclusivePlugins.length === 0 && exclusiveSkills.length === 0 && (
            <p className="text-xs text-muted-foreground">{t("settings.appSettings.extEmpty")}</p>
          )}
        </div>
        <p className="mt-2 text-xs text-muted-foreground">{t("settings.appSettings.extHint")}</p>
      </div>

      {/* 记忆：按 APP 的记忆体系（聊天与编程不同） */}
      <div className="rounded-xl border p-4">
        <h3 className="text-sm font-semibold">{t("settings.appSettings.memoryTitle")}</h3>
        <p className="mt-1 text-xs text-muted-foreground">
          {t(
            appId === "coding"
              ? "settings.appSettings.memoryCoding"
              : "settings.appSettings.memoryChat",
          )}
        </p>
        {profile?.memory && (
          <Badge className="mt-2 text-[10px]">{t("settings.appSettings.memoryBucket", { bucket: profile.memory })}</Badge>
        )}
      </div>

      {/* 工作区 */}
      <div className="rounded-xl border p-4">
        <h3 className="text-sm font-semibold">{t("settings.appSettings.workspaceTitle")}</h3>
        <p className="mt-1 text-xs text-muted-foreground">
          {t("settings.appSettings.workspaceDesc", {
            dir: config?.working_dir ?? "",
          })}
        </p>
        {appId === "coding" && (
          <p className="mt-1 text-xs text-muted-foreground">{t("settings.appSettings.workspaceCoding")}</p>
        )}
      </div>
    </section>
  );
}
