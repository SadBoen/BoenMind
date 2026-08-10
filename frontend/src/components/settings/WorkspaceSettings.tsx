/**
 * 工作文件夹设置：指定 BoenMind 文件浏览区的根目录。
 */
import { useState } from "react";
import { FolderOpen, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toast } from "sonner";
import { useAppStore } from "@/stores/app-store";

export function WorkspaceSettings() {
  const config = useAppStore((s) => s.config);
  const saveConfig = useAppStore((s) => s.saveConfig);
  const navigateDir = useAppStore((s) => s.navigateDir);
  const [path, setPath] = useState(config?.working_dir ?? "");

  if (!config) {
    return <p className="text-sm text-muted-foreground">加载配置中…</p>;
  }

  const save = async () => {
    const trimmed = path.trim().replace(/\/+$/, "");
    if (!trimmed) {
      toast.error("路径不能为空");
      return;
    }
    try {
      await saveConfig({ ...config, working_dir: trimmed });
      toast.success("工作文件夹已更新");
      await navigateDir("");
    } catch (err) {
      toast.error(`保存失败: ${String(err)}`);
    }
  };

  return (
    <section className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">工作文件夹</h2>
        <p className="text-sm text-muted-foreground">
          文件浏览区默认展示此目录。首次启动自动创建 <code className="rounded bg-muted px-1">~/BoenMind</code>。
        </p>
      </div>

      <div className="space-y-2">
        <Label>文件夹路径</Label>
        <div className="flex gap-2">
          <div className="relative flex-1">
            <FolderOpen size={15} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" />
            <Input value={path} onChange={(e) => setPath(e.target.value)} className="pl-8 font-mono text-xs" placeholder="~/BoenMind" />
          </div>
          <Button onClick={() => void save()}>
            <RefreshCw size={14} className="mr-1" />
            保存
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          当前目录：<span className="font-mono">{config.working_dir}</span>
        </p>
      </div>
    </section>
  );
}
