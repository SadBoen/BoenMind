/**
 * 双 DE 壳入口：启动画面 → 经典软件界面（默认）或桌面壳。
 * 界面模式由用户选择持久化（boenmind.viewMode），两壳共享同一 store、
 * 同一批应用内容组件（APPS 注册表），后端零改动——架构 §四·B
 * "前端壳多套并存"的实作验证。
 */
import { useCallback, useEffect, useState } from "react";
import { Desktop } from "@/components/desktop/Desktop";
import { ClassicShell } from "@/components/classic/ClassicShell";
import { BootScreen } from "@/components/desktop/BootScreen";
import { applyFontScale, fontScale } from "@/lib/appearance";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useTranslation } from "react-i18next";
import { KeyRound } from "lucide-react";
import { onUnauthorized, setAuthToken } from "@/api/client";
import { useAppStore } from "@/stores/app-store";

export default function App() {
  const [booted, setBooted] = useState(false);
  const viewMode = useAppStore((s) => s.viewMode);
  const refreshHealth = useAppStore((s) => s.refreshHealth);
  const loadConfig = useAppStore((s) => s.loadConfig);
  const loadSessions = useAppStore((s) => s.loadSessions);

  // 启动加载：健康状态（轮询）+ 配置 + 会话列表
  useEffect(() => {
    // 全局字体档位（软件形态外观设置；rem 布局随根字号缩放）
    applyFontScale(fontScale());
    void refreshHealth();
    void loadConfig();
    void loadSessions();
    const timer = setInterval(() => void refreshHealth(), 5000);
    return () => clearInterval(timer);
  }, [refreshHealth, loadConfig, loadSessions]);

  // 启动画面结束才渲染界面壳（经典软件界面默认；桌面模式为 OS 形态入口）
  const finishBoot = useCallback(() => setBooted(true), []);

  return (
    <div className="h-screen w-screen overflow-hidden bg-background text-foreground">
      {booted ? (viewMode === "desktop" ? <Desktop /> : <ClassicShell />) : <BootScreen onFinish={finishBoot} />}
      <TokenGate />
    </div>
  );
}

/**
 * 访问令牌门（服务器部署设置 BOENMIND_TOKEN 后生效）：
 * 任意 /api 请求返回 401 unauthorized 时弹出输入框，保存后自动重载数据。
 * 桌面版后端不设令牌，此组件永不弹出。
 */
function TokenGate() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [token, setToken] = useState("");

  useEffect(() => {
    onUnauthorized(() => setOpen(true));
    return () => onUnauthorized(null);
  }, []);

  const submit = () => {
    setAuthToken(token);
    setOpen(false);
    setToken("");
    // 令牌已保存：重新拉取全部数据（health 轮询在 App 启动 effect 里持续进行）
    const store = useAppStore.getState();
    void store.refreshHealth();
    void store.loadConfig();
    void store.loadSessions();
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent showCloseButton={false}>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <KeyRound size={15} />
            {t("auth.title")}
          </DialogTitle>
          <DialogDescription>{t("auth.desc")}</DialogDescription>
        </DialogHeader>
        <Input
          type="password"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          placeholder={t("auth.placeholder")}
          className="font-mono"
          autoFocus
          onKeyDown={(e) => e.key === "Enter" && submit()}
        />
        <DialogFooter showCloseButton>
          <Button onClick={submit}>{t("common.save")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
