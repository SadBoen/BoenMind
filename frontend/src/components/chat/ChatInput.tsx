/**
 * 聊天输入区：自动增高文本域，Enter 发送 / Shift+Enter 换行。
 */
import { useState } from "react";
import { ArrowUp } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { useAppStore } from "@/stores/app-store";

export function ChatInput() {
  const streaming = useAppStore((s) => s.streaming);
  const sendMessage = useAppStore((s) => s.sendMessage);
  const [text, setText] = useState("");

  const canSend = text.trim().length > 0 && !streaming;

  const submit = async () => {
    if (!canSend) return;
    const value = text.trim();
    setText("");
    await sendMessage(value);
  };

  return (
    <div className="shrink-0 border-t p-3">
      <div className="mx-auto max-w-3xl">
        <div className="flex items-end gap-2 rounded-xl border bg-background p-2 shadow-sm focus-within:ring-1 focus-within:ring-ring">
          <Textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                void submit();
              }
            }}
            placeholder="输入消息，Enter 发送，Shift+Enter 换行"
            rows={1}
            className="max-h-40 min-h-[2.25rem] resize-none border-0 bg-transparent p-1.5 text-sm shadow-none focus-visible:ring-0"
            disabled={streaming}
          />
          <Button
            size="icon"
            className="h-8 w-8 shrink-0 rounded-lg"
            onClick={() => void submit()}
            disabled={!canSend}
            title="发送"
          >
            <ArrowUp size={16} />
          </Button>
        </div>
        <p className="mt-1.5 text-center text-[10px] text-muted-foreground">
          内容由 AI 生成，请核实重要信息
        </p>
      </div>
    </div>
  );
}
