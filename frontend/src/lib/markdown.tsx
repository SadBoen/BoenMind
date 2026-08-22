import { useState } from "react";
import DOMPurify from "dompurify";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import hljs from "highlight.js/lib/core";
import typescript from "highlight.js/lib/languages/typescript";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import bash from "highlight.js/lib/languages/bash";
import markdownLang from "highlight.js/lib/languages/markdown";
import css from "highlight.js/lib/languages/css";
import { IconCopy } from "./icons";

hljs.registerLanguage("ts", typescript);
hljs.registerLanguage("tsx", typescript);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("js", javascript);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("json", json);
hljs.registerLanguage("bash", bash);
hljs.registerLanguage("sh", bash);
hljs.registerLanguage("md", markdownLang);
hljs.registerLanguage("markdown", markdownLang);
hljs.registerLanguage("css", css);

function highlight(code: string, lang?: string): string {
  try {
    if (lang && hljs.getLanguage(lang)) return hljs.highlight(code, { language: lang }).value;
    return hljs.highlightAuto(code).value;
  } catch {
    return code;
  }
}

function CodeBlock({ lang, code }: { lang: string; code: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="code-block">
      <div className="code-head">
        <span>{lang || "text"}</span>
        <button
          type="button"
          className="icon-btn"
          onClick={async () => {
            await navigator.clipboard.writeText(code);
            setCopied(true);
            window.setTimeout(() => setCopied(false), 1500);
          }}
        >
          <IconCopy /> {copied ? "Copied!" : "复制"}
        </button>
      </div>
      <pre>
        {/* hljs 输出经 DOMPurify 白名单（仅 span.class）再注入：不依赖高亮器内部
            转义兜底模型输出（不可信面）。 */}
        <code
          dangerouslySetInnerHTML={{
            __html: DOMPurify.sanitize(highlight(code, lang), {
              ALLOWED_TAGS: ["span"],
              ALLOWED_ATTR: ["class"],
            }),
          }}
        />
      </pre>
    </div>
  );
}

export function MarkdownView({ children, streaming }: { children: string; streaming?: boolean }) {
  if (streaming) {
    return (
      <pre className="think-body" style={{ fontFamily: "var(--font-ui-stack)", fontSize: "var(--font-body)", lineHeight: 1.75, color: "var(--fg)" }}>
        {children}
      </pre>
    );
  }
  return (
    <div className="prose">
      <Markdown
        remarkPlugins={[remarkGfm]}
        components={{
          code({ className, children: c, ...rest }) {
            const text = String(c).replace(/\n$/, "");
            const lang = /language-(\w+)/.exec(className ?? "")?.[1] ?? "";
            const inline = !className && !text.includes("\n");
            if (inline) {
              return (
                <code className={className} {...rest}>
                  {c}
                </code>
              );
            }
            return <CodeBlock lang={lang} code={text} />;
          },
        }}
      >
        {children}
      </Markdown>
    </div>
  );
}

