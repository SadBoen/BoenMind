/**
 * 插件设置：列表 + 启停开关 + 本地安装。
 * 插件基于 pi 扩展机制（QuickJS 直接加载 TypeScript），无需转 Rust。
 */
import { useEffect, useState } from "react";
import { Puzzle, Plus, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { toast } from "sonner";
import { api, type PluginInfo } from "@/api/client";

export function PluginsSettings() {
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [installPath, setInstallPath] = useState("");

  const load = async () => {
    setLoading(true);
    try {
      setPlugins(await api.listPlugins());
    } catch (err) {
      toast.error(`加载插件列表失败: ${String(err)}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const toggle = async (plugin: PluginInfo) => {
    try {
      await api.setPlugin(plugin.id, !plugin.enabled);
      toast.success(plugin.enabled ? `已禁用 ${plugin.name}` : `已启用 ${plugin.name}`);
      await load();
    } catch (err) {
      toast.error(`操作失败: ${String(err)}`);
    }
  };

  const install = async () => {
    const path = installPath.trim();
    if (!path) {
      toast.error("请输入插件路径（.ts 文件或插件目录）");
      return;
    }
    try {
      await api.installPlugin(path);
      toast.success("插件已安装，可在列表中启用");
      setInstallPath("");
      await load();
    } catch (err) {
      toast.error(`安装失败: ${String(err)}`);
    }
  };

  return (
    <section className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">插件</h2>
        <p className="text-sm text-muted-foreground">
          基于 pi 扩展机制（QuickJS 运行时直接加载 TypeScript 扩展，无需编译或转 Rust）。
          启用后，插件注册的工具与命令对 AI 助手立即生效。
        </p>
      </div>

      {/* 安装 */}
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Plus size={15} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={installPath}
            onChange={(e) => setInstallPath(e.target.value)}
            placeholder="本地插件路径：/path/to/plugin.ts 或插件目录"
            className="pl-8 font-mono text-xs"
          />
        </div>
        <Button variant="outline" onClick={() => void install()}>
          安装
        </Button>
        <Button variant="ghost" size="icon" onClick={() => void load()} title="刷新">
          <RefreshCw size={15} className={loading ? "animate-spin" : ""} />
        </Button>
      </div>

      {/* 列表 */}
      {loading ? (
        <p className="text-sm text-muted-foreground">加载中…</p>
      ) : plugins.length === 0 ? (
        <div className="rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground">
          还没有插件。内置示例（hello / bookmark）会在首次启动时自动安装。
        </div>
      ) : (
        <div className="space-y-3">
          {plugins.map((plugin) => (
            <div key={plugin.id} className="flex items-center justify-between gap-3 rounded-xl border p-4">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <Puzzle size={15} className="shrink-0 text-muted-foreground" />
                  <h3 className="font-semibold">{plugin.name}</h3>
                  {plugin.builtin && (
                    <Badge variant="secondary" className="text-[10px]">
                      内置示例
                    </Badge>
                  )}
                  <Badge variant="outline" className="text-[10px] font-normal">
                    {plugin.kind === "single" ? "单文件" : "清单目录"}
                  </Badge>
                </div>
                <p className="mt-1 truncate text-xs text-muted-foreground">{plugin.description}</p>
              </div>
              <Switch checked={plugin.enabled} onCheckedChange={() => void toggle(plugin)} />
            </div>
          ))}
        </div>
      )}

      <p className="text-xs text-muted-foreground">
        提示：插件在创建新对话会话时加载；启用/禁用后新对话生效。社区插件（pi.dev/packages）中
        无原生依赖的扩展可直接复制到 <code className="rounded bg-muted px-1">~/.boenmind/extensions/</code> 后刷新安装。
      </p>
    </section>
  );
}
