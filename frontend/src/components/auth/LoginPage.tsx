/**
 * 登录页（公网站点 UI 门）：只密码、无用户名。
 *
 * 浏览器必须先过本页才能进聊天/编程/WIKI/设置；密码默认 `adminadmin`，
 * 可在设置中心「安全」页修改。桌面壳（Tauri）本地使用不经过本页。
 * 登录成功后回调 onAuthed（App 层切换主界面）。
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { KeyRound, Loader2 } from "lucide-react";
import { api, setUiSession } from "@/api/client";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export function LoginPage({ onAuthed }: { onAuthed: () => void }) {
  const { t } = useTranslation();
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (!password || busy) return;
    setBusy(true);
    setError(null);
    try {
      const res = await api.authLogin(password);
      setUiSession(res.token);
      onAuthed();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full w-full items-center justify-center bg-background">
      <div className="w-full max-w-sm px-6">
        <div className="mb-8 flex flex-col items-center gap-3">
          <div className="flex h-14 w-14 items-center justify-center rounded-2xl border bg-muted/40">
            <KeyRound size={26} className="text-primary" />
          </div>
          <h1 className="text-xl font-semibold">BoenMind</h1>
          <p className="text-center text-sm text-muted-foreground">{t("auth.loginDesc")}</p>
        </div>

        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="login-password">{t("auth.passwordLabel")}</Label>
            <Input
              id="login-password"
              type="password"
              value={password}
              onChange={(e) => {
                setPassword(e.target.value);
                setError(null);
              }}
              placeholder={t("auth.passwordPlaceholder")}
              autoFocus
              autoComplete="current-password"
              disabled={busy}
            />
          </div>
          {error && (
            <p className="text-sm text-destructive" role="alert">
              {t("auth.wrongPassword")}
            </p>
          )}
          <Button type="submit" disabled={busy || !password} className="w-full">
            {busy ? (
              <>
                <Loader2 size={15} className="mr-1.5 animate-spin" />
                {t("auth.loggingIn")}
              </>
            ) : (
              t("auth.login")
            )}
          </Button>
        </form>
      </div>
    </div>
  );
}
