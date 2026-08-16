/**
 * 公共 Markdown 渲染组件（2026-08-16 WIKI 应用抽取）：
 * 合并 MessageItem / FilePreview 两处私有实现——remark-gfm 表格 + rehype-highlight
 * 代码高亮 + 新窗口链接 + 滚动代码块。prose 排版由调用方按场景选择：
 * 聊天消息用 `text-sm`（紧凑），Wiki 阅读器用 `prose`（正文排版）。
 */
import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";

export function Markdown({ content }: { content: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      rehypePlugins={[rehypeHighlight]}
      components={{
        pre: ({ children }) => (
          <pre className="overflow-x-auto rounded-lg border bg-muted/60 p-3 text-xs">
            {children}
          </pre>
        ),
        a: ({ children, href, ...props }) => {
          const safe =
            !href ||
            href.startsWith("#") ||
            href.startsWith("http://") ||
            href.startsWith("https://") ||
            href.startsWith("mailto:");
          return (
            <a
              {...props}
              href={safe ? href : undefined}
              target="_blank"
              rel="noreferrer"
              className="text-primary underline"
            >
              {children}
            </a>
          );
        },
      }}
    >
      {content}
    </ReactMarkdown>
  );
}
