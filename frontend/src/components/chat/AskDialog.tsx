/**
 * ask_user 提问弹窗（2026-08-17 新增工具）：模型调用 ask_user 时由聊天流
 * 事件触发，用户填写回答后回传后端。无回答时后端 60s 超时按失败收尾
 * （模型可见），本组件用同款计时器自动关闭。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { MessageCircleQuestion } from "lucide-react";
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
import { useAppStore } from "@/stores/app-store";

/** 与后端 PERMISSION_TIMEOUT（ask_user 共用）保持一致 */
const ASK_TIMEOUT_MS = 60_000;

export function AskDialog() {
  const { t } = useTranslation();
  const pending = useAppStore((s) => s.pendingAsk);
  const respond = useAppStore((s) => s.respondAsk);
  const dismiss = useAppStore((s) => s.dismissAsk);
  const [answer, setAnswer] = useState("");

  // 新提问重置输入框（弹窗复用组件实例时防残留）
  useEffect(() => {
    if (pending) setAnswer("");
  }, [pending]);

  // 超时自动关闭：后端 60s 无回答即按失败收尾，弹窗同步收起
  useEffect(() => {
    if (!pending) return;
    const timer = setTimeout(dismiss, ASK_TIMEOUT_MS);
    return () => clearTimeout(timer);
  }, [pending, dismiss]);

  if (!pending) return null;

  return (
    <Dialog open onOpenChange={() => {}}>
      <DialogContent className="max-w-md" showCloseButton={false}>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <MessageCircleQuestion size={17} className="text-primary" />
            {t("chat.ask.title")}
          </DialogTitle>
          <DialogDescription className="text-left">{pending.question}</DialogDescription>
        </DialogHeader>
        <Input
          value={answer}
          onChange={(e) => setAnswer(e.target.value)}
          placeholder={t("chat.ask.placeholder")}
          autoFocus
          onKeyDown={(e) => {
            if (e.key === "Enter" && answer.trim()) void respond(answer.trim());
          }}
        />
        <DialogFooter className="gap-2 sm:justify-end">
          <Button variant="ghost" onClick={() => void respond("")}>
            {t("common.cancel")}
          </Button>
          <Button disabled={!answer.trim()} onClick={() => void respond(answer.trim())}>
            {t("chat.ask.submit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
