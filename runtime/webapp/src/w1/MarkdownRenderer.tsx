import React, { useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Check, Copy } from "lucide-react";

/**
 * MarkdownRenderer: 聊天消息的 Markdown 渲染器
 * - 支持 GFM（表格、删除线、任务列表）
 * - 支持代码块（CodeBlock），带有快捷复制代码按钮与语言标注
 * - 针对四套主题自适应样式与排版
 */
export function MarkdownRenderer({ content }: { content: string }) {
  return (
    <div className="markdown-body">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          code({ className, children, ...props }) {
            const match = /language-(\w+)/.exec(className || "");
            const isInline = !match && !String(children).includes("\n");
            if (isInline) {
              return (
                <code className="inline-code" {...props}>
                  {children}
                </code>
              );
            }
            return (
              <CodeBlock language={match ? match[1] : ""}>
                {String(children).replace(/\n$/, "")}
              </CodeBlock>
            );
          },
          table({ children }) {
            return (
              <div className="my-2 overflow-x-auto rounded-md border border-border/80">
                <table className="w-full border-collapse text-left text-[12.5px]">
                  {children}
                </table>
              </div>
            );
          },
          th({ children }) {
            return (
              <th className="border-b border-border bg-muted/50 px-3 py-1.5 font-semibold text-foreground">
                {children}
              </th>
            );
          },
          td({ children }) {
            return (
              <td className="border-b border-border/50 px-3 py-1.5 text-foreground/90">
                {children}
              </td>
            );
          },
          a({ href, children }) {
            return (
              <a
                href={href}
                target="_blank"
                rel="noreferrer"
                className="text-primary underline underline-offset-2 hover:opacity-80"
              >
                {children}
              </a>
            );
          },
          ul({ children }) {
            return <ul className="my-1.5 list-disc pl-5 space-y-1">{children}</ul>;
          },
          ol({ children }) {
            return <ol className="my-1.5 list-decimal pl-5 space-y-1">{children}</ol>;
          },
          blockquote({ children }) {
            return (
              <blockquote className="my-2 border-l-2 border-primary/50 pl-3 italic text-muted-foreground">
                {children}
              </blockquote>
            );
          },
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}

function CodeBlock({
  language,
  children,
}: {
  language: string;
  children: string;
}) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(children);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      /* ignore */
    }
  };

  return (
    <div className="code-block my-2 overflow-hidden rounded-lg border border-border/80 bg-muted/40 font-mono text-[12px]">
      <div className="flex items-center justify-between border-b border-border/50 bg-muted/60 px-3 py-1 text-muted-foreground text-[11px]">
        <span className="font-semibold uppercase tracking-wider">
          {language || "code"}
        </span>
        <button
          onClick={handleCopy}
          className="flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors hover:bg-background/80 hover:text-foreground"
          title="复制代码"
        >
          {copied ? (
            <>
              <Check className="size-3 text-emerald-500" />
              <span className="text-emerald-500 text-[10.5px]">已复制</span>
            </>
          ) : (
            <>
              <Copy className="size-3" />
              <span className="text-[10.5px]">复制</span>
            </>
          )}
        </button>
      </div>
      <pre className="overflow-x-auto p-3 leading-relaxed text-foreground">
        <code>{children}</code>
      </pre>
    </div>
  );
}
