/**
 * Skill 设置：列表 + 启停开关 + 卸载 + skills.sh 随机抽取安装 + 本地安装。
 * Skill = SKILL.md 目录（含脚本），启用后同步到 pi 的 skills 目录，
 * 由 agent 会话自动注入（新对话生效）。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Dices, Loader2, Plus, RefreshCw, Trash2, Wand2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { toast } from "sonner";
import { api, type SkillCandidate, type SkillInfo } from "@/api/client";

const RANDOM_COUNT = 5;

export function SkillsSettings() {
  const { t } = useTranslation();
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [candidates, setCandidates] = useState<SkillCandidate[]>([]);
  const [fetching, setFetching] = useState(false);
  const [installingId, setInstallingId] = useState<string | null>(null);
  const [installPath, setInstallPath] = useState("");

  const load = async () => {
    setLoading(true);
    try {
      setSkills(await api.listSkills());
    } catch (err) {
      toast.error(t("settings.skills.loadFailed", { error: String(err) }));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const toggle = async (skill: SkillInfo) => {
    try {
      await api.setSkill(skill.id, !skill.enabled);
      toast.success(
        skill.enabled
          ? t("settings.skills.toggledOff", { name: skill.name })
          : t("settings.skills.toggledOn", { name: skill.name }),
        { description: t("settings.skills.newSessionHint") },
      );
      await load();
    } catch (err) {
      toast.error(t("settings.skills.toggleFailed", { error: String(err) }));
    }
  };

  const uninstall = async (skill: SkillInfo) => {
    if (!window.confirm(t("settings.skills.uninstallConfirm", { name: skill.name }))) return;
    try {
      await api.uninstallSkill(skill.id);
      toast.success(t("settings.skills.uninstalled", { name: skill.name }));
      await load();
    } catch (err) {
      toast.error(t("settings.skills.uninstallFailed", { error: String(err) }));
    }
  };

  const fetchRandom = async () => {
    setFetching(true);
    try {
      const list = await api.randomSkills(RANDOM_COUNT);
      setCandidates(list);
      if (list.length === 0) {
        toast.error(t("settings.skills.randomEmpty"));
      }
    } catch (err) {
      toast.error(t("settings.skills.randomFailed", { error: String(err) }));
    } finally {
      setFetching(false);
    }
  };

  const installCandidate = async (c: SkillCandidate) => {
    setInstallingId(c.skill_id);
    try {
      await api.installSkill({ owner: c.owner, repo: c.repo, skill_id: c.skill_id });
      toast.success(t("settings.skills.installed", { name: c.name }));
      setCandidates((prev) => prev.filter((x) => x.skill_id !== c.skill_id));
      await load();
    } catch (err) {
      toast.error(t("settings.skills.installFailed", { error: String(err) }));
    } finally {
      setInstallingId(null);
    }
  };

  const installLocal = async () => {
    const path = installPath.trim();
    if (!path) {
      toast.error(t("settings.skills.pathRequired"));
      return;
    }
    try {
      const info = await api.installSkill({ path });
      toast.success(t("settings.skills.installed", { name: info.name }));
      setInstallPath("");
      await load();
    } catch (err) {
      toast.error(t("settings.skills.installFailed", { error: String(err) }));
    }
  };

  return (
    <section className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">{t("settings.skills.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("settings.skills.desc")}</p>
      </div>

      {/* skills.sh 随机抽取 */}
      <div className="rounded-xl border p-4">
        <div className="flex items-center justify-between gap-3">
          <div className="min-w-0">
            <h3 className="flex items-center gap-1.5 text-sm font-semibold">
              <Wand2 size={14} className="shrink-0 text-muted-foreground" />
              {t("settings.skills.randomTitle")}
            </h3>
            <p className="mt-0.5 text-xs text-muted-foreground">
              {t("settings.skills.randomDesc", { count: RANDOM_COUNT })}
            </p>
          </div>
          <Button variant="outline" size="sm" onClick={() => void fetchRandom()} disabled={fetching}>
            {fetching ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <Dices size={14} />
            )}
            {t("settings.skills.randomButton")}
          </Button>
        </div>
        {candidates.length > 0 && (
          <div className="mt-3 space-y-2">
            {candidates.map((c) => (
              <div key={`${c.owner}/${c.repo}/${c.skill_id}`} className="flex items-center justify-between gap-3 rounded-lg border p-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <h4 className="truncate text-sm font-medium">{c.name}</h4>
                    <Badge variant="outline" className="shrink-0 text-[10px] font-normal">
                      {c.owner}/{c.repo}
                    </Badge>
                  </div>
                  <p className="mt-0.5 line-clamp-2 text-xs text-muted-foreground">{c.description}</p>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  className="shrink-0"
                  disabled={installingId === c.skill_id}
                  onClick={() => void installCandidate(c)}
                >
                  {installingId === c.skill_id ? (
                    <Loader2 size={13} className="animate-spin" />
                  ) : (
                    <Plus size={13} />
                  )}
                  {t("settings.skills.install")}
                </Button>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* 本地安装 */}
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Plus size={15} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={installPath}
            onChange={(e) => setInstallPath(e.target.value)}
            placeholder={t("settings.skills.installPlaceholder")}
            className="pl-8 font-mono text-xs"
          />
        </div>
        <Button variant="outline" onClick={() => void installLocal()}>
          {t("settings.skills.install")}
        </Button>
        <Button variant="ghost" size="icon" onClick={() => void load()} title={t("common.refresh")}>
          <RefreshCw size={15} className={loading ? "animate-spin" : ""} />
        </Button>
      </div>

      {/* 已装列表 */}
      {loading ? (
        <p className="text-sm text-muted-foreground">{t("common.loading")}</p>
      ) : skills.length === 0 ? (
        <div className="rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground">
          {t("settings.skills.empty")}
        </div>
      ) : (
        <div className="space-y-3">
          {skills.map((skill) => (
            <div key={skill.id} className="flex items-center justify-between gap-3 rounded-xl border p-4">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <Wand2 size={15} className="shrink-0 text-muted-foreground" />
                  <h3 className="font-semibold">{skill.name}</h3>
                  <Badge variant="secondary" className="text-[10px]">
                    {skill.source === "registry"
                      ? t("settings.skills.fromRegistry")
                      : t("settings.skills.fromLocal")}
                  </Badge>
                  {skill.owner && (
                    <Badge variant="outline" className="text-[10px] font-normal">
                      {skill.owner}/{skill.repo}
                    </Badge>
                  )}
                </div>
                <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">{skill.description}</p>
              </div>
              <div className="flex shrink-0 items-center gap-3">
                <Switch checked={skill.enabled} onCheckedChange={() => void toggle(skill)} />
                <Button
                  variant="ghost"
                  size="icon"
                  className="text-muted-foreground hover:text-destructive"
                  title={t("settings.skills.uninstall")}
                  onClick={() => void uninstall(skill)}
                >
                  <Trash2 size={15} />
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      <p className="text-xs text-muted-foreground">
        {t("settings.skills.tip")}
      </p>
    </section>
  );
}
