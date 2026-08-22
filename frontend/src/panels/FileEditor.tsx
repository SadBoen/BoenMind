import { useEffect } from "react";
import { findNode } from "../lib/files";
import { IconSave } from "../lib/icons";
import { useStore } from "../store";
import { toast } from "../lib/toast";

export function FileEditor() {
  const { state, dispatch } = useStore();
  const file = state.openFilePath ? findNode(state.files, state.openFilePath) : null;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        if (state.openFileDirty) {
          dispatch({ type: "save-file" });
          toast.success("已保存");
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dispatch, state.openFileDirty]);

  if (!file) {
    return <div className="empty">双击文本文件在这里打开。</div>;
  }

  if (file.kind === "binary") {
    return <div className="binary-empty">无法预览</div>;
  }

  if (file.kind === "image") {
    const src =
      file.content?.startsWith("<svg")
        ? `data:image/svg+xml;utf8,${encodeURIComponent(file.content)}`
        : file.dataUrl ?? "";
    // React 元素渲染（src/alt 自动转义）；不再拼接 HTML 字符串（属性逃逸面）。
    return (
      <div className="preview-html">
        <div className="editor-head">
          <span className="editor-path">{file.path}</span>
        </div>
        <img alt={file.name} src={src} />
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", minWidth: 0 }}>
      <div className="editor-head">
        <span className="editor-path" title={file.path}>
          {state.openFileDirty && <span className="dirty">•</span>}
        </span>
        <button
          type="button"
          className="icon-btn"
          disabled={!state.openFileDirty}
          aria-label="保存"
          title={file.path}
          onClick={() => {
            dispatch({ type: "save-file" });
            toast.success("已保存");
          }}
        >
          <IconSave />
        </button>
      </div>
      <textarea
        className="editor-area"
        value={state.openFileDraft}
        onChange={(e) => {
          dispatch({ type: "set-draft", value: e.target.value });
        }}
        spellCheck={false}
      />
    </div>
  );
}
