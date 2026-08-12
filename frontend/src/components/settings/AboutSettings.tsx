/**
 * 关于与更新：版本信息、检查更新、热升级（不重装、不退出程序）。
 *
 * 升级走自更新链路（bm-server /api/updates/*，全部用户手动触发，无自动检查）：
 * 1. 检查更新 → 2. 点升级：下载 → 验签 → 落盘（managed：runtime 目录 /
 *    standalone：替换自身）→ 3. 重启后端（桌面版调壳 backend_restart 命令；
 *    Linux 部署后端自身 exec，PID 不变）→ 4. 轮询 health 检测新版本就绪 →
 *    刷新页面完成。应用窗口全程不关。
 */

import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckCircle2, Download, RefreshCw, Rocket, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Badge } from "@/components/ui/badge";
import { toast } from "sonner";
import { useAppStore } from "@/stores/app-store";
import { api } from "@/api/client";

/// 应用版本兜底（health 未加载时展示）；加载后以后端版本为准（单源）
const APP_VERSION = "0.1.1";
/// 升级后等待后端重启的轮询上限（秒）：超时提示手动刷新
const RESTART_WAIT_SECONDS = 60;

type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "available"; version: string; notes?: string }
  | { status: "none" }
  | { status: "downloading" }
  | { status: "restarting"; version: string }
  | { status: "error"; message: string };

export function AboutSettings() {
  const { t } = useTranslation();
  const health = useAppStore((s) => s.health);
  // 应用版本以 health.version（后端）为准，常量仅作启动兜底
  const appVersion = health?.version ?? APP_VERSION;
  const [state, setState] = useState<UpdateState>({ status: "idle" });
  // 轮询句柄：卸载/重进时清理，防升级完成前组件销毁后继续 setState
  const pollRef = useRef<number | null>(null);

  // 桌面环境检测（Tauri 注入的全局）：managed 模式需调壳的 backend_restart；
  // Web 版（Linux 部署）由后端自身 exec 重启，同样支持热升级
  const isDesktop = typeof window !== "undefined" && Boolean(
    (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__,
  );

  const clearPoll = () => {
    if (pollRef.current !== null) {
      window.clearInterval(pollRef.current);
      pollRef.current = null;
    }
  };

  const checkForUpdates = async () => {
    setState({ status: "checking" });
    try {
      const info = await api.checkUpdate();
      if (!info.latest) {
        setState({ status: "none" });
        toast.success(t("settings.about.latest"));
        return;
      }
      setState({ status: "available", version: info.latest.version, notes: info.latest.notes });
    } catch (err) {
      setState({ status: "error", message: String(err) });
    }
  };

  const upgradeNow = async () => {
    setState({ status: "downloading" });
    try {
      // 下载 → 验签 → 落盘（有运行中任务时后端会拒绝，错误详情直接展示）
      const result = await api.applyUpdate();
      setState({ status: "restarting", version: result.version });
      if (result.mode === "managed" && isDesktop) {
        // 桌面版：壳 kill 旧后端并按 runtime 最新版重新拉起（窗口不关）
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("backend_restart");
      } else {
        // Linux 部署：后端 300ms 后 exec 新版本（PID 不变，systemd 无感知）
        await api.restartUpdate();
      }
      waitForNewBackend(result.version);
    } catch (err) {
      clearPoll();
      setState({ status: "error", message: String(err) });
    }
  };

  /// 轮询 health：新版后端就绪（版本号已变为目标版本）→ 刷新页面完成升级
  const waitForNewBackend = (targetVersion: string) => {
    const startedAt = Date.now();
    pollRef.current = window.setInterval(async () => {
      try {
        const current = await api.health();
        if (current.version === targetVersion) {
          clearPoll();
          // 升级完成：整页刷新（重新拉取配置/会话/任务状态）
          window.location.reload();
          return;
        }
      } catch {
        // 后端切换中（旧进程已停/新进程未起），继续等
      }
      if (Date.now() - startedAt > RESTART_WAIT_SECONDS * 1000) {
        clearPoll();
        setState({ status: "error", message: t("settings.about.restartTimeout") });
      }
    }, 2000);
  };

  return (
    <section className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">{t("settings.about.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("settings.about.desc")}</p>
      </div>

      <div className="space-y-3 rounded-xl border p-4">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="font-semibold">BoenMind</h3>
            <p className="text-xs text-muted-foreground">{t("settings.about.tagline")}</p>
          </div>
          <Badge variant="secondary">v{appVersion}</Badge>
        </div>
        <div className="flex flex-wrap gap-x-6 gap-y-1 text-xs text-muted-foreground">
          <span>{t("settings.about.backendV", { version: health?.version ?? "-" })}</span>
          <span>
            {health
              ? t("settings.about.providersCount", { count: health.providers })
              : t("settings.about.backendOffline")}
          </span>
          <span>{isDesktop ? t("settings.about.desktop") : t("settings.about.web")}</span>
        </div>
        <p className="text-xs text-muted-foreground">{t("settings.about.updateViaReleases")}</p>
      </div>

      {/* 更新状态区（全部用户手动触发） */}
      <div className="space-y-3">
        {state.status === "idle" && (
          <Button onClick={() => void checkForUpdates()}>
            <RefreshCw size={15} className="mr-1" />
            {t("settings.about.checkUpdate")}
          </Button>
        )}
        {state.status === "checking" && (
          <Button disabled>
            <RefreshCw size={15} className="mr-1 animate-spin" />
            {t("settings.about.checking")}
          </Button>
        )}
        {state.status === "none" && (
          <div className="flex items-center gap-2 text-sm text-emerald-600">
            <CheckCircle2 size={16} />
            {t("settings.about.latestVersion", { version: appVersion })}
            <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => void checkForUpdates()}>
              {t("settings.about.recheck")}
            </Button>
          </div>
        )}
        {state.status === "available" && (
          <div className="rounded-xl border p-4">
            <div className="flex items-center gap-2">
              <Rocket size={16} className="text-primary" />
              <h4 className="font-semibold">{t("settings.about.available", { version: state.version })}</h4>
            </div>
            {state.notes && (
              <p className="mt-2 whitespace-pre-wrap rounded-lg bg-muted/50 p-3 text-xs text-muted-foreground">
                {state.notes}
              </p>
            )}
            <div className="mt-3 flex gap-2">
              <Button size="sm" onClick={() => void upgradeNow()}>
                <Download size={14} className="mr-1" />
                {t("settings.about.upgradeNow")}
              </Button>
              <Button size="sm" variant="outline" onClick={() => setState({ status: "idle" })}>
                {t("settings.about.later")}
              </Button>
            </div>
          </div>
        )}
        {state.status === "downloading" && (
          <div className="space-y-2 rounded-xl border p-4">
            <p className="flex items-center gap-2 text-sm">
              <Download size={15} className="animate-pulse text-primary" />
              {t("settings.about.downloading")}
            </p>
            {/* 单次下载请求无进度事件：不确定动画 */}
            <Progress value={null} className="animate-pulse" />
          </div>
        )}
        {state.status === "restarting" && (
          <div className="flex items-center gap-2 rounded-xl border p-4 text-sm">
            <RefreshCw size={16} className="animate-spin text-primary" />
            <span className="flex-1">
              {t("settings.about.restarting", { version: state.version })}
            </span>
          </div>
        )}
        {state.status === "error" && (
          <div className="flex items-center gap-2 rounded-xl border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">
            <XCircle size={16} />
            <span className="flex-1 break-all">{state.message}</span>
            <Button size="sm" variant="outline" onClick={() => setState({ status: "idle" })}>
              {t("common.close")}
            </Button>
          </div>
        )}
      </div>
    </section>
  );
}
