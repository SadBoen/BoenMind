/**
 * 关于与更新：版本信息、检查更新、下载进度、重启提示。
 * 升级走 Tauri updater（仅桌面版可用）：检测 GitHub Release → 后台下载 → 提示重启。
 */
import { useState } from "react";
import { CheckCircle2, Download, RefreshCw, Rocket, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Badge } from "@/components/ui/badge";
import { toast } from "sonner";
import { useAppStore } from "@/stores/app-store";

const APP_VERSION = "0.1.0";

type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "available"; version: string; notes?: string }
  | { status: "none" }
  | { status: "downloading"; percent: number }
  | { status: "installed"; version: string }
  | { status: "error"; message: string };

export function AboutSettings() {
  const health = useAppStore((s) => s.health);
  const [state, setState] = useState<UpdateState>({ status: "idle" });

  // 桌面环境检测（Tauri 注入的全局）
  const isDesktop = typeof window !== "undefined" && Boolean(
    (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__,
  );

  const checkForUpdates = async () => {
    if (!isDesktop) {
      toast.info("网页版无需更新，请使用桌面版体验自动升级");
      return;
    }
    setState({ status: "checking" });
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (!update) {
        setState({ status: "none" });
        toast.success("已是最新版本");
        return;
      }
      setState({ status: "available", version: update.version, notes: update.body ?? undefined });
    } catch (err) {
      setState({ status: "error", message: String(err) });
    }
  };

  const downloadAndInstall = async (version: string) => {
    setState({ status: "downloading", percent: 0 });
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const { relaunch } = await import("@tauri-apps/plugin-process");
      const update = await check();
      if (!update || update.version !== version) {
        setState({ status: "error", message: "更新状态已变化，请重新检查" });
        return;
      }
      let downloaded = 0;
      await update.downloadAndInstall((event) => {
        if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          setState({ status: "downloading", percent: downloaded });
        }
      });
      setState({ status: "installed", version });
      toast("更新已安装，重启后生效", {
        description: "点击「立即重启」完成升级",
        action: {
          label: "立即重启",
          onClick: () => void relaunch(),
        },
        duration: 60000,
      });
    } catch (err) {
      setState({ status: "error", message: String(err) });
    }
  };

  const relaunchNow = async () => {
    try {
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (err) {
      toast.error(`重启失败: ${String(err)}`);
    }
  };

  return (
    <section className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">关于</h2>
        <p className="text-sm text-muted-foreground">版本信息与自动更新</p>
      </div>

      <div className="space-y-3 rounded-xl border p-4">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="font-semibold">BoenMind</h3>
            <p className="text-xs text-muted-foreground">
              个人知识管理 · LLM-WIKI + 个人 Agent
            </p>
          </div>
          <Badge variant="secondary">v{APP_VERSION}</Badge>
        </div>
        <div className="flex flex-wrap gap-x-6 gap-y-1 text-xs text-muted-foreground">
          <span>后端：v{health?.version ?? "-"}</span>
          <span>{health ? `${health.providers} 个提供商` : "后端未连接"}</span>
          <span>{isDesktop ? "桌面版（Tauri）" : "网页版"}</span>
        </div>
        <p className="text-xs text-muted-foreground">
          自动更新通过 GitHub Releases 分发，macOS 上后台下载、重启后生效。
        </p>
      </div>

      {/* 更新状态区 */}
      <div className="space-y-3">
        {state.status === "idle" && (
          <Button onClick={() => void checkForUpdates()}>
            <RefreshCw size={15} className="mr-1" />
            检查更新
          </Button>
        )}
        {state.status === "checking" && (
          <Button disabled>
            <RefreshCw size={15} className="mr-1 animate-spin" />
            检查中…
          </Button>
        )}
        {state.status === "none" && (
          <div className="flex items-center gap-2 text-sm text-emerald-600">
            <CheckCircle2 size={16} />
            已是最新版本 v{APP_VERSION}
            <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => void checkForUpdates()}>
              重新检查
            </Button>
          </div>
        )}
        {state.status === "available" && (
          <div className="rounded-xl border p-4">
            <div className="flex items-center gap-2">
              <Rocket size={16} className="text-primary" />
              <h4 className="font-semibold">发现新版本 v{state.version}</h4>
            </div>
            {state.notes && (
              <p className="mt-2 whitespace-pre-wrap rounded-lg bg-muted/50 p-3 text-xs text-muted-foreground">
                {state.notes}
              </p>
            )}
            <div className="mt-3 flex gap-2">
              <Button size="sm" onClick={() => void downloadAndInstall(state.version)}>
                <Download size={14} className="mr-1" />
                下载并安装
              </Button>
              <Button size="sm" variant="outline" onClick={() => setState({ status: "idle" })}>
                稍后
              </Button>
            </div>
          </div>
        )}
        {state.status === "downloading" && (
          <div className="space-y-2 rounded-xl border p-4">
            <p className="flex items-center gap-2 text-sm">
              <Download size={15} className="animate-pulse text-primary" />
              正在后台下载更新… 已下载 {(state.percent / 1024 / 1024).toFixed(1)} MB
            </p>
            <Progress value={100} className="animate-pulse" />
            <p className="text-xs text-muted-foreground">下载完成后会提示重启，不影响当前使用</p>
          </div>
        )}
        {state.status === "installed" && (
          <div className="flex items-center gap-2 rounded-xl border border-emerald-600/30 bg-emerald-500/5 p-4 text-sm">
            <CheckCircle2 size={16} className="text-emerald-600" />
            <span className="flex-1">
              v{state.version} 已安装，重启后生效
            </span>
            <Button size="sm" onClick={() => void relaunchNow()}>
              立即重启
            </Button>
          </div>
        )}
        {state.status === "error" && (
          <div className="flex items-center gap-2 rounded-xl border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">
            <XCircle size={16} />
            <span className="flex-1 break-all">{state.message}</span>
            <Button size="sm" variant="outline" onClick={() => setState({ status: "idle" })}>
              关闭
            </Button>
          </div>
        )}
      </div>
    </section>
  );
}
