/**
 * 文件预览：Markdown / 纯文本 / 代码 / 图片 / PDF。
 */
import { useEffect, useState } from "react";
import { ArrowLeft, FileQuestion, Loader2 } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { api, type FileEntry } from "@/api/client";
import { useAppStore } from "@/stores/app-store";

type PreviewData = {
  name: string;
  mime: string;
  kind: "text" | "binary";
  content: string;
  size: number;
};

export function FilePreview({ file }: { file: FileEntry }) {
  const openFile = useAppStore((s) => s.openFile);
  const [data, setData] = useState<PreviewData | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setData(null);
    setError(null);
    let cancelled = false;
    api
      .readFile(file.path)
      .then((d) => !cancelled && setData(d))
      .catch((err) => !cancelled && setError(String(err)));
    return () => {
      cancelled = true;
    };
  }, [file.path]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* 预览头部 */}
      <div className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          title="返回列表"
          onClick={() => openFile(null)}
        >
          <ArrowLeft size={14} />
        </Button>
        <span className="min-w-0 flex-1 truncate text-xs font-medium" title={file.path}>
          {file.name}
        </span>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="p-3">
          {error ? (
            <p className="py-8 text-center text-xs text-destructive">{error}</p>
          ) : !data ? (
            <div className="flex justify-center py-10">
              <Loader2 size={18} className="animate-spin text-muted-foreground" />
            </div>
          ) : data.kind === "text" ? (
            data.mime === "text/markdown" ? (
              <article className="prose prose-sm dark:prose-invert max-w-none break-words">
                <ReactMarkdown
                  remarkPlugins={[remarkGfm]}
                  rehypePlugins={[rehypeHighlight]}
                  components={{
                    pre: ({ children }) => (
                      <pre className="overflow-x-auto rounded-lg border bg-muted/60 p-3 text-xs">
                        {children}
                      </pre>
                    ),
                  }}
                >
                  {data.content}
                </ReactMarkdown>
              </article>
            ) : (
              <pre className="whitespace-pre-wrap break-words rounded-lg border bg-muted/40 p-3 font-mono text-xs leading-relaxed">
                {data.content}
              </pre>
            )
          ) : data.mime.startsWith("image/") ? (
            <img
              src={`data:${data.mime};base64,${data.content}`}
              alt={file.name}
              className="mx-auto max-w-full rounded-lg"
            />
          ) : data.mime === "application/pdf" ? (
            <iframe
              src={`data:application/pdf;base64,${data.content}`}
              title={file.name}
              className="h-[70vh] w-full rounded-lg border"
            />
          ) : (
            <div className="flex flex-col items-center gap-2 py-10 text-muted-foreground">
              <FileQuestion size={28} />
              <p className="text-xs">暂不支持预览该文件类型（{data.mime}）</p>
            </div>
          )}
        </div>
      </ScrollArea>
    </div>
  );
}
