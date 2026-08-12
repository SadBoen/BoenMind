/**
 * 专家团队功能描述页：加载 public/docs/expert-team.md 并渲染。
 * 阶段 0 形态——功能尚未实现，此页是构思/规划文档的载体，
 * 后续构思直接编辑 MD 文件即可，无需改代码。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";

const DOC_URL = "/docs/expert-team.md";

export function ExpertTeamDocs() {
  const { t } = useTranslation();
  const [doc, setDoc] = useState<string | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    let cancelled = false;
    fetch(DOC_URL)
      .then((res) => (res.ok ? res.text() : Promise.reject(new Error(String(res.status)))))
      .then((text) => {
        if (!cancelled) setDoc(text);
      })
      .catch(() => {
        if (!cancelled) setError(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (error) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        {t("team.docLoadFailed")}
      </div>
    );
  }
  if (doc === null) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        {t("common.loading")}
      </div>
    );
  }

  return (
    <div className="h-full min-w-0 overflow-y-auto bg-background">
      <div className="mx-auto max-w-3xl px-6 py-6">
        {/* 注意：react-markdown v10 不接受 className prop（传了会崩溃），
            样式类须放在外层容器（与 MessageItem 同模式） */}
        <div className="prose prose-sm dark:prose-invert max-w-none break-words">
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            rehypePlugins={[rehypeHighlight]}
            components={{
              pre: ({ children }) => (
                <pre className="overflow-x-auto rounded-lg border bg-muted/60 p-3 text-xs">{children}</pre>
              ),
            }}
          >
            {doc}
          </ReactMarkdown>
        </div>
      </div>
    </div>
  );
}
