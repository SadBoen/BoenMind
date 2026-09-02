// W7 设置中心 · 关于页:版本/平台/数据目录 + 检查更新 + 一键升级。
// 升级 = 下载 release 包→校验→换装→服务自动重启;完成后轮询 /health,
// 服务回来即带时间戳强刷(取新前端,破缓存)。发新版本必须用户明说(铁规矩)。
import { useEffect, useState } from "react";
import { DownloadIcon, Loader2Icon, RefreshCwIcon } from "lucide-react";
import { api } from "./api";
import { Button } from "@/components/ui/button";

type CheckResult = Awaited<ReturnType<typeof api.about.checkUpdate>>;

export function AboutPage() {
  const [info, setInfo] = useState<{
    version: string;
    platform: string;
    dataDir: string;
    repo: string;
  } | null>(null);
  const [check, setCheck] = useState<CheckResult | null>(null);
  const [checking, setChecking] = useState(false);
  const [upgrading, setUpgrading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.about.get().then(setInfo).catch((e) => setError(String(e instanceof Error ? e.message : e)));
  }, []);

  const runCheck = async () => {
    setChecking(true);
    setError(null);
    setMessage(null);
    try {
      const r = await api.about.checkUpdate();
      setCheck(r);
      if (!r.ok) setError(r.error ?? "检查失败");
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setChecking(false);
    }
  };

  const runUpgrade = async () => {
    if (
      !window.confirm(
        `升级到 ${check?.latest}?服务将自动重启(进行中的回合会中断),完成后本页自动刷新。`,
      )
    )
      return;
    setUpgrading(true);
    setError(null);
    setMessage("正在下载并换装……服务即将重启");
    try {
      const r = await api.about.applyUpdate();
      if (!r.ok) {
        setError(r.note);
        setUpgrading(false);
        return;
      }
      setMessage(r.note);
      // 轮询 /health,服务就绪(可能已是新版本)后强刷取新前端
      const deadline = Date.now() + 120_000;
      const poll = async () => {
        try {
          const h = await fetch("/health").then((x) => x.json());
          if (h?.ok) {
            window.location.href = "/?upgrade=" + Date.now();
            return;
          }
        } catch {
          /* 未就绪,继续等 */
        }
        if (Date.now() < deadline) setTimeout(poll, 1500);
      };
      setTimeout(poll, 3000);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
      setUpgrading(false);
    }
  };

  const updateAvailable = check?.ok && check.updateAvailable;

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h2 className="text-[15px] font-semibold">关于</h2>
        <p className="text-muted-foreground text-[12.5px]">
          BoenMind —— 个人生态的 AI Runtime / AI OS。
        </p>
      </div>

      {info ? (
        <div className="bg-card flex flex-col gap-1.5 rounded-xl border p-3 text-[12.5px]">
          <div className="flex gap-2">
            <span className="text-muted-foreground w-24 shrink-0">当前版本</span>
            <span className="font-mono" data-slot="about-version">
              v{info.version}
            </span>
          </div>
          <div className="flex gap-2">
            <span className="text-muted-foreground w-24 shrink-0">平台</span>
            <span className="font-mono">{info.platform}</span>
          </div>
          <div className="flex gap-2">
            <span className="text-muted-foreground w-24 shrink-0">数据目录</span>
            <span className="truncate font-mono text-[11.5px]">{info.dataDir}</span>
          </div>
          <div className="flex gap-2">
            <span className="text-muted-foreground w-24 shrink-0">更新源</span>
            <span className="font-mono text-[11.5px]">{info.repo}</span>
          </div>
        </div>
      ) : null}

      <div className="flex items-center gap-2">
        <Button variant="outline" size="sm" disabled={checking || upgrading} onClick={() => void runCheck()}>
          {checking ? <Loader2Icon className="animate-spin" /> : <RefreshCwIcon />}
          检查更新
        </Button>
        {updateAvailable ? (
          <Button size="sm" disabled={upgrading} onClick={() => void runUpgrade()}>
            {upgrading ? <Loader2Icon className="animate-spin" /> : <DownloadIcon />}
            一键升级到 {check?.latest}
          </Button>
        ) : null}
      </div>

      {check?.ok ? (
        <div
          className="rounded-lg border px-3 py-2 text-[12.5px]"
          data-check={updateAvailable ? "available" : "latest"}
        >
          {updateAvailable ? (
            <span>
              发现新版本 <span className="font-mono">{check.latest}</span>(当前 v{check.current}),
              可一键升级。
            </span>
          ) : (
            <span>
              已是最新版本(v{check.current},线上 latest{" "}
              <span className="font-mono">{check.latest}</span>)。
            </span>
          )}
          {check.notes ? (
            <details className="text-muted-foreground mt-1.5">
              <summary className="cursor-pointer text-[12px]">发行说明</summary>
              <pre className="mt-1 overflow-x-auto whitespace-pre-wrap text-[11.5px]">
                {check.notes}
              </pre>
            </details>
          ) : null}
        </div>
      ) : null}

      {message ? (
        <div className="notice-success">
          {message}
        </div>
      ) : null}
      {error ? (
        <div className="notice-error">
          {error}
        </div>
      ) : null}
    </div>
  );
}
