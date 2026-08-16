/**
 * 设置中心「安全」页（公网站点 UI 登录门）：修改访问密码 + 登出当前会话。
 *
 * - 只密码、无用户名；默认密码 loveBM@86，首次登录后建议在此修改。
 * - 改密需当前密码正确 + 会话有效；新密码最短 4 位。
 * - 会话失效（login required）时 App 登录门自动复位回登录页。
 * - 桌面壳（Tauri）本地使用不启用登录门，本页在桌面版设置菜单中隐藏。
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { KeyRound, LogOut } from "lucide-react";
import { api } from "@/api/client";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toast } from "sonner";

export function SecuritySettings() {
  const { t } = useTranslation();
  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (!currentPassword || !newPassword || busy) return;
    if (newPassword !== confirm) {
      toast.error(t("settings.security.passwordMismatch"));
      return;
    }
    setBusy(true);
    try {
      await api.changePassword({ current_password: currentPassword, new_password: newPassword });
      toast.success(t("settings.security.passwordChanged"));
      setCurrentPassword("");
      setNewPassword("");
      setConfirm("");
    } catch (e) {
      // login required 由 client.ts 触发 onUiUnauthorized → 登录门复位
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const logout = () => {
    // 登出由 App 层处理（清会话 token + 回登录页）；这里只需发起
    void api.authLogout().catch(() => {});
    // 通知 App 登录门复位（经 window 事件避免循环依赖）
    window.dispatchEvent(new CustomEvent("boenmind:logout"));
  };

  return (
    <section className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">{t("settings.security.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("settings.security.desc")}</p>
      </div>

      <div className="space-y-3 rounded-xl border p-4">
        <div className="flex items-center gap-2">
          <KeyRound size={16} className="text-muted-foreground" />
          <h3 className="font-semibold">{t("settings.security.changePassword")}</h3>
        </div>
        <p className="text-xs text-muted-foreground">{t("settings.security.changePasswordDesc")}</p>

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="sec-current">{t("settings.security.currentPassword")}</Label>
          <Input
            id="sec-current"
            type="password"
            value={currentPassword}
            onChange={(e) => setCurrentPassword(e.target.value)}
            autoComplete="current-password"
            className="max-w-sm"
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="sec-new">{t("settings.security.newPassword")}</Label>
          <Input
            id="sec-new"
            type="password"
            value={newPassword}
            onChange={(e) => setNewPassword(e.target.value)}
            autoComplete="new-password"
            className="max-w-sm"
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="sec-confirm">{t("settings.security.confirmPassword")}</Label>
          <Input
            id="sec-confirm"
            type="password"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            autoComplete="new-password"
            className="max-w-sm"
          />
        </div>
        <Button onClick={() => void submit()} disabled={busy || !currentPassword || !newPassword}>
          {t("settings.security.savePassword")}
        </Button>
      </div>

      <div className="space-y-3 rounded-xl border p-4">
        <div className="flex items-center gap-2">
          <LogOut size={16} className="text-muted-foreground" />
          <h3 className="font-semibold">{t("settings.security.session")}</h3>
        </div>
        <p className="text-xs text-muted-foreground">{t("settings.security.sessionDesc")}</p>
        <Button variant="outline" onClick={logout}>
          <LogOut size={14} className="mr-1.5" />
          {t("settings.security.logout")}
        </Button>
      </div>
    </section>
  );
}
