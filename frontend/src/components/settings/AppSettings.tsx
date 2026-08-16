/**
 * 每软件 APP 设置页（设置架构 §五）：专家绑定 / 独有扩展（勾选编辑）/
 * 记忆 / 工作区。所有带 LLM 的 APP（chat/coding/…）共用此模板，
 * appId 由注册表传入。底层 LLM 交互引擎所有 APP 共用一套
 * （单源 config.toml 的 [apps.<id>] 段）。
 *
 * 2026-08-16 设计定调（作用域操作面）：APP 视角 = 勾选式——列出全部
 * 插件与 SKILL，勾上 = 仅本 APP 生效（作用域含 appId），不勾 = 公共；
 * 数量多时用名称筛选。扩展视角 = 插件/Skills 页徽标就地改（ScopePicker）。
 * 默认公共，只在例外处动手——设置量与"例外数"成正比。
 */
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Puzzle, Search, Wand2 } from "lucide-react";
import { toast } from "sonner";
import { api, type AppProfile, type ExpertDef, type PluginInfo, type SkillInfo } from "@/api/client";
import { useAppStore } from "@/stores/app-store";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";

export function AppSettings({ appId }: { appId: string }) {
  const { t } = useTranslation();
  const config = useAppStore((s) => s.config);
  const loadConfig = useAppStore((s) => s.loadConfig);
  const [experts, setExperts] = useState<ExpertDef[]>([]);
  const [expert, setExpert] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [filter, setFilter] = useState("");

  useEffect(() => {
    setExpert(config?.apps?.[appId]?.expert ?? null);
  }, [config, appId]);

  useEffect(() => {
    api
      .listExperts()
      .then(setExperts)
      .catch(() => setExperts([]));
  }, []);

  // 全部插件/SKILL（勾选 = 作用域含本 APP）
  useEffect(() => {
    api
      .listPlugins()
      .then(setPlugins)
      .catch(() => setPlugins([]));
    api
      .listSkills()
      .then(setSkills)
      .catch(() => setSkills([]));
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

  /** 勾选切换某扩展的作用域：勾上 = 作用域加本 APP；取消 = 移除
   * （空 = 公共）。写 config 的 plugin_scopes/skill_scopes（引擎已生效）。 */
  const toggleScope = async (
    id: string,
    kind: "plugin" | "skill",
    current: string[] | undefined,
    checked: boolean,
  ) => {
    const next = new Set(current ?? []);
    if (checked) next.add(appId);
    else next.delete(appId);
    const scopes = [...next];
    try {
      if (kind === "plugin") await api.setPluginScope(id, scopes);
      else await api.setSkillScope(id, scopes);
      await loadConfig();
    } catch (err) {
      toast.error(t("settings.appSettings.scopeFailed", { error: String(err) }));
    }
  };

  const appName = t(`settings.menu.app${appId === "chat" ? "Chat" : "Coding"}`);

  // 勾选清单（全部扩展，按名称筛选）
  const filteredPlugins = useMemo(() => {
    const kw = filter.trim().toLowerCase();
    return plugins.filter((p) => !kw || p.id.toLowerCase().includes(kw) || p.name.toLowerCase().includes(kw));
  }, [plugins, filter]);
  const filteredSkills = useMemo(() => {
    const kw = filter.trim().toLowerCase();
    return skills.filter((s) => !kw || s.id.toLowerCase().includes(kw) || s.name.toLowerCase().includes(kw));
  }, [skills, filter]);

  const isExclusive = (kind: "plugin" | "skill", id: string) => {
    const map = kind === "plugin" ? config?.plugin_scopes : config?.skill_scopes;
    return (map?.[id] ?? []).includes(appId);
  };

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
                  {e.name}
                  {e.model ? ` · ${e.model}` : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {saving && <Loader2 size={14} className="animate-spin text-muted-foreground" />}
        </div>
        <p className="mt-2 text-xs text-muted-foreground">{t("settings.appSettings.expertHint")}</p>
      </div>

      {/* 扩展：勾选 = 仅本 APP（作用域含 appId）；不勾 = 公共 */}
      <div className="rounded-xl border p-4">
        <h3 className="flex items-center gap-1.5 text-sm font-semibold">
          <Puzzle size={14} className="text-muted-foreground" />
          {t("settings.appSettings.extTitle")}
        </h3>
        <p className="text-xs text-muted-foreground">{t("settings.appSettings.extDesc")}</p>
        <div className="relative mt-3">
          <Search size={13} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder={t("settings.appSettings.extFilter")}
            className="h-8 pl-8 text-xs"
          />
        </div>
        <div className="mt-2 max-h-64 space-y-1 overflow-y-auto rounded-md border p-2">
          {filteredPlugins.length === 0 && filteredSkills.length === 0 && (
            <p className="py-2 text-center text-xs text-muted-foreground">{t("settings.appSettings.extEmpty")}</p>
          )}
          {filteredPlugins.map((p) => (
            <label
              key={`p:${p.id}`}
              className="flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-sm hover:bg-accent/50"
            >
              <input
                type="checkbox"
                checked={isExclusive("plugin", p.id)}
                onChange={(e) => void toggleScope(p.id, "plugin", config?.plugin_scopes?.[p.id], e.target.checked)}
                className="size-4 shrink-0 accent-primary"
              />
              <span className="min-w-0 flex-1 truncate">{p.id}</span>
              <Badge variant="outline" className="shrink-0 text-[10px] font-normal">
                {t("settings.appSettings.kindPlugin")}
              </Badge>
            </label>
          ))}
          {filteredSkills.map((s) => (
            <label
              key={`s:${s.id}`}
              className="flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-sm hover:bg-accent/50"
            >
              <input
                type="checkbox"
                checked={isExclusive("skill", s.id)}
                onChange={(e) => void toggleScope(s.id, "skill", config?.skill_scopes?.[s.id], e.target.checked)}
                className="size-4 shrink-0 accent-primary"
              />
              <Wand2 size={13} className="shrink-0 text-muted-foreground" />
              <span className="min-w-0 flex-1 truncate">{s.id}</span>
              <Badge variant="outline" className="shrink-0 text-[10px] font-normal">
                {t("settings.appSettings.kindSkill")}
              </Badge>
            </label>
          ))}
        </div>
        <p className="mt-2 text-xs text-muted-foreground">{t("settings.appSettings.extHint")}</p>
      </div>

      {/* 记忆：绑定专家后按专家记忆桶（自动）；未绑定 = 全局 */}
      <div className="rounded-xl border p-4">
        <h3 className="text-sm font-semibold">{t("settings.appSettings.memoryTitle")}</h3>
        <p className="mt-1 text-xs text-muted-foreground">
          {t(
            appId === "coding"
              ? "settings.appSettings.memoryCoding"
              : "settings.appSettings.memoryChat",
          )}
        </p>
        <p className="mt-1 text-xs text-muted-foreground">{t("settings.appSettings.memoryAuto")}</p>
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
