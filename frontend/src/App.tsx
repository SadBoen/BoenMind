/**
 * 壳入口：经典软件界面（唯一形态）。
 * 桌面形态已退役（2026-08-16，用户拍板：全删除，留切换开关占位），
 * 渲染恒为 ClassicShell。
 *
 * 皮肤层（2026-08-16）：根容器 relative + 背景层 z-0 + 内容 z-10——
 * 玻璃皮肤下 --background 半透明，背景层（图片/渐变）透出成为玻璃材质的内容物。
 */
import { useEffect, useState } from "react";
import { ClassicShell } from "@/components/classic/ClassicShell";
import { SkinBackground } from "@/components/skin/SkinBackground";
import { applyAccent, applyFontScale, applyReduceMotion, fontScale } from "@/lib/appearance";
import { applySkin } from "@/lib/skin";
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
    // 全局外观：字体档位 + 强调色/减少动画 + 皮肤（挂载恢复）
    applyFontScale(fontScale());
    const s = useAppStore.getState();
    applyAccent(s.accent);
    applyReduceMotion(s.reduceMotion);
    applySkin(s.skin, s.skinParams);
    void refreshHealth();
    void loadConfig();
    void loadSessions();
    const timer = setInterval(() => void refreshHealth(), 5000);
    return () => clearInterval(timer);
  }, [refreshHealth, loadConfig, loadSessions]);

  return (
    <div className="relative h-screen w-screen overflow-hidden bg-background text-foreground">
      <SkinBackground />
      <div className="relative z-10 h-full">
        <ClassicShell />
      </div>
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
