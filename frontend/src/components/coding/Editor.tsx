/**
 * 编程壳编辑器（M2）：文本文件 → 可编辑 textarea + 保存（POST 写文件端点）；
 * 二进制/图片 → FilePreview 只读渲染（复用原预览组件）。
 * 未保存标记：内容与磁盘不一致时显示圆点；保存成功后刷新文件树。
 */
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Loader2, Save, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { toast } from "sonner";
import { api } from "@/api/client";
import { useAppStore } from "@/stores/app-store";
import { FilePreview } from "@/components/files/FilePreview";

type EditorData = {
  name: string;
  path: string;
  mime: string;
  kind: "text" | "binary";
  content: string;
  size: number;
};

export function Editor() {
  const { t } = useTranslation();
  const previewFile = useAppStore((s) => s.previewFile);
  const refreshFiles = useAppStore((s) => s.refreshFiles);
  const [data, setData] = useState<EditorData | null>(null);
  const [value, setValue] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const loadingRef = useRef(0);

  // 打开文件 → 读取内容（取消保护：快速切换文件时不串台）
  useEffect(() => {
    const id = ++loadingRef.current;
    setData(null);
    setError(null);
    if (!previewFile) return;
    api
      .readFile(previewFile.path)
      .then((d) => {
        if (loadingRef.current !== id) return;
        setData(d as EditorData);
        setValue(d.content);
      })
      .catch((err) => {
        if (loadingRef.current === id) setError(String(err));
      });
  }, [previewFile]);

  if (!previewFile) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
        <FileIcon className="h-10 w-10 opacity-40" />
        <p className="text-xs">{t("coding.editor.empty")}</p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <p className="text-xs text-destructive">{error}</p>
      </div>
    );
  }

  if (!data) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 size={18} className="animate-spin text-muted-foreground" />
      </div>
    );
  }

  const dirty = data.kind === "text" && value !== data.content;
  const save = async () => {
    if (!previewFile || data.kind !== "text") return;
    setSaving(true);
    try {
      await api.writeFile(previewFile.path, value);
      setData((d) => (d ? { ...d, content: value, size: value.length } : d));
      toast.success(t("coding.editor.saved"));
      // 保存后刷新文件树（大小/时间变化；不动当前打开的编辑器）
      void refreshFiles();
    } catch (err) {
      toast.error(String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex h-full min-w-0 flex-col bg-background">
      {/* 编辑器头部：文件名 + 保存 */}
      <div className="flex h-11 shrink-0 items-center gap-2 border-b px-3">
        <span className="min-w-0 flex-1 truncate text-sm font-medium" title={previewFile.path}>
          {dirty && <span className="mr-1.5 inline-block h-2 w-2 rounded-full bg-amber-500" />}
          {previewFile.name}
        </span>
        {data.kind === "text" ? (
          <Button
            size="sm"
            variant="outline"
            className="h-7 gap-1 text-xs"
            disabled={!dirty || saving}
            onClick={() => void save()}
          >
            {saving ? <Loader2 size={12} className="animate-spin" /> : <Save size={12} />}
            {t("coding.editor.save")}
          </Button>
        ) : null}
        <Button size="sm" variant="ghost" className="h-7 px-2" onClick={() => useAppStore.getState().openFile(null)} title={t("common.close")}>
          <X size={13} />
        </Button>
      </div>

      {data.kind === "binary" ? (
        <FilePreview file={previewFile} />
      ) : (
        <ScrollArea className="min-h-0 flex-1">
          <textarea
            value={value}
            onChange={(e) => setValue(e.target.value)}
            spellCheck={false}
            className="h-full w-full resize-none bg-transparent p-3 font-mono text-xs leading-relaxed outline-none"
          />
        </ScrollArea>
      )}

      {/* 保存成功提示（dirty 消失即隐含；加一行状态栏更直观） */}
      {data.kind === "text" && !dirty && !saving && (
        <div className="flex shrink-0 items-center gap-1 border-t px-3 py-1 text-[10px] text-muted-foreground">
          <Check size={11} className="text-green-600" />
          {t("coding.editor.synced")}
        </div>
      )}
    </div>
  );
}

function FileIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className={className}>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6" />
    </svg>
  );
}
