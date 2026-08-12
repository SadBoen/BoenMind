/**
 * 插件权限询问弹窗：插件请求危险能力（exec/env 等）时由聊天流事件触发。
 * 按钮：允许 / 拒绝 / 总是允许。
 * 决策记忆在上游（extension-permissions.json，跨会话生效、插件版本更新后
 * 重新询问），这里只负责把用户选择转发给后端。
 * 无选择时后端 60s 超时 fail-closed 拒绝，本组件用同款计时器自动关闭
 * （与后端 PERMISSION_TIMEOUT 保持一致）。
 */
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { ShieldAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useAppStore } from "@/stores/app-store";

/** 与后端 permission.rs 的 PERMISSION_TIMEOUT 保持一致 */
const PERMISSION_TIMEOUT_MS = 60_000;

export function PermissionDialog() {
  const { t } = useTranslation();
  const pending = useAppStore((s) => s.pendingPermission);
  const respond = useAppStore((s) => s.respondPermission);
  const dismiss = useAppStore((s) => s.dismissPermission);

  // 超时自动关闭：后端 60s 无决策即 fail-closed，弹窗同步收起
  useEffect(() => {
    if (!pending) return;
    const timer = setTimeout(dismiss, PERMISSION_TIMEOUT_MS);
    return () => clearTimeout(timer);
  }, [pending, dismiss]);

  if (!pending) return null;

  return (
    <Dialog open onOpenChange={() => {}}>
      <DialogContent className="max-w-md" showCloseButton={false}>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldAlert size={17} className="text-amber-500" />
            {t("chat.permission.title")}
          </DialogTitle>
          <DialogDescription className="text-left">
            <div className="space-y-2">
              <p>{pending.message}</p>
              <p className="rounded-lg bg-muted px-2.5 py-1.5 font-mono text-[11px]">
                {pending.extensionId ?? t("chat.permission.unknownExtension")}
                <span className="mx-1.5 text-muted-foreground">/</span>
                {pending.capability}
              </p>
              <p className="text-xs text-muted-foreground">{t("chat.permission.hint")}</p>
            </div>
          </DialogDescription>
        </DialogHeader>
        <DialogFooter className="gap-2 sm:justify-start">
          <Button variant="outline" onClick={() => void respond(false, false)}>
            {t("chat.permission.denyOnce")}
          </Button>
          <Button
            variant="outline"
            className="text-amber-600"
            onClick={() => void respond(true, true)}
          >
            {t("chat.permission.allowAlways")}
          </Button>
          <Button className="ml-auto" onClick={() => void respond(true, false)}>
            {t("chat.permission.allowOnce")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
