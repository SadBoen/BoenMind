// 用户消息视图(#22 拆分:自 thread.tsx 机械移入):
// 原文展示 + 行内编辑重发(开辟分支)+ 复制。
import { MessagePrimitive, useAuiState } from "@assistant-ui/react";
import { User, Pencil, Copy, Check } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { useBoenmindApprovals } from "../runtime";

export function UserMessage() {
  const messageIndex = useAuiState((s) => s.message.index);
  const { editAndBranchMessage } = useBoenmindApprovals();
  const [isEditing, setIsEditing] = useState(false);
  const [editText, setEditText] = useState("");
  const [copied, setCopied] = useState(false);

  return (
    <MessagePrimitive.Root className="msg user">
      <div className="msg-header">
        <User size={13} className="text-primary" />
        <span>我</span>
      </div>
      <div className="content">
        <MessagePrimitive.Parts>
          {({ part }) => {
            if (part.type !== "text") return null;
            if (isEditing) {
              return (
                <div className="msg-inline-editor">
                  <textarea
                    className="msg-inline-textarea"
                    value={editText}
                    onChange={(e) => setEditText(e.target.value)}
                    autoFocus
                    rows={3}
                  />
                  <div className="flex items-center justify-end gap-2">
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-7 px-2 text-[12px]"
                      onClick={() => setIsEditing(false)}
                    >
                      取消
                    </Button>
                    <Button
                      size="sm"
                      className="h-7 px-3 text-[12px]"
                      onClick={async () => {
                        setIsEditing(false);
                        if (messageIndex !== undefined && editText.trim()) {
                          await editAndBranchMessage(messageIndex, editText.trim());
                        }
                      }}
                    >
                      提交并生成分支
                    </Button>
                  </div>
                </div>
              );
            }
            return (
              <div className="group/content relative flex flex-col gap-1">
                <span className="select-text whitespace-pre-wrap">{part.text}</span>
                <div className="msg-action-bar justify-end pt-1">
                  <button
                    className="msg-action-btn"
                    title="编辑本条消息并开辟分支"
                    onClick={() => {
                      setEditText(part.text);
                      setIsEditing(true);
                    }}
                  >
                    <Pencil size={12} />
                  </button>
                  <button
                    className="msg-action-btn"
                    title="复制消息"
                    onClick={async () => {
                      await navigator.clipboard.writeText(part.text);
                      setCopied(true);
                      setTimeout(() => setCopied(false), 1500);
                    }}
                  >
                    {copied ? <Check size={12} className="text-emerald-500" /> : <Copy size={12} />}
                  </button>
                </div>
              </div>
            );
          }}
        </MessagePrimitive.Parts>
      </div>
    </MessagePrimitive.Root>
  );
}
