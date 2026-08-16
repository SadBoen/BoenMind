/**
 * 壳入口：经典软件界面（唯一形态）。
 * 桌面形态已退役（2026-08-16，用户拍板：全删除，留切换开关占位），
 * viewMode 状态保留供开关回显，渲染恒为 ClassicShell。
 */
import { useEffect, useState } from "react";
import { ClassicShell } from "@/components/classic/ClassicShell";
import { applyAccent, applyFontScale, applyReduceMotion, fontScale } from "@/lib/appearance";
import { useAppStore } from "@/stores/app-store";
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

export default function App() {
  const refreshHealth = useAppStore((s) => s.refreshHealth);
  const loadConfig = useAppStore((s) => s.loadConfig);
  const loadSessions = useAppStore((s) => s.loadSessions);

  // 启动加载：健康状态（轮询）+ 配置 + 会话列表
  useEffect(() => {
    // 全局外观：字体档位 + 强调色/减少动画（资深外观设置；挂载恢复）
    applyFontScale(fontScale());
    const s = useAppStore.getState();
    applyAccent(s.accent);
    applyReduceMotion(s.reduceMotion);
    void refreshHealth();
    void loadConfig();
    void loadSessions();
    const timer = setInterval(() => void refreshHealth(), 5000);
    return () => clearInterval(timer);
  }, [refreshHealth, loadConfig, loadSessions]);

  return (
    <div className="h-screen w-screen overflow-hidden bg-background text-foreground">
      <ClassicShell />
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
