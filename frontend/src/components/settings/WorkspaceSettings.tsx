/**
 * 工作文件夹设置：指定 BoenMind 文件浏览区的根目录。
 */
import { useEffect, useState } from "react";
import { useTranslation, Trans } from "react-i18next";
import { FolderOpen, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toast } from "sonner";
import { useAppStore } from "@/stores/app-store";

export function WorkspaceSettings() {
  const { t } = useTranslation();
  const config = useAppStore((s) => s.config);
  const saveConfig = useAppStore((s) => s.saveConfig);
  const navigateDir = useAppStore((s) => s.navigateDir);
  const [path, setPath] = useState("");

  // config 异步加载/保存完成后同步到输入框（初始 useState 只取一次会拿到空串）
  useEffect(() => {
    if (config) setPath(config.working_dir);
  }, [config]);

  if (!config) {
    return <p className="text-sm text-muted-foreground">{t("settings.workspace.loadingConfig")}</p>;
  }

  const save = async () => {
    const trimmed = path.trim().replace(/\/+$/, "");
    if (!trimmed) {
      toast.error(t("settings.workspace.pathRequired"));
      return;
    }
    try {
      await saveConfig({ ...config, working_dir: trimmed });
      toast.success(t("settings.workspace.saved"));
      await navigateDir("");
    } catch (err) {
      toast.error(t("settings.workspace.saveFailed", { error: String(err) }));
    }
  };

  return (
    <section className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">{t("settings.workspace.title")}</h2>
        <p className="text-sm text-muted-foreground">
          <Trans
            i18nKey="settings.workspace.desc"
            components={{ code: <code className="rounded bg-muted px-1" /> }}
          />
        </p>
      </div>

      <div className="space-y-2">
        <Label>{t("settings.workspace.pathLabel")}</Label>
        <div className="flex gap-2">
          <div className="relative flex-1">
            <FolderOpen size={15} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" />
            <Input value={path} onChange={(e) => setPath(e.target.value)} className="pl-8 font-mono text-xs" placeholder="~/BoenMind" />
          </div>
          <Button onClick={() => void save()}>
            <RefreshCw size={14} className="mr-1" />
            {t("common.save")}
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          {t("settings.workspace.currentDir")}
          <span className="font-mono">{config.working_dir}</span>
        </p>
      </div>
    </section>
  );
}
