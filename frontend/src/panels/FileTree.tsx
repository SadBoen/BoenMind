import { useMemo, useState } from "react";
import { nodesAt } from "../lib/files";
import { IconChevron, IconDrawer, IconFile, IconFolder, IconImage } from "../lib/icons";
import { useStore } from "../store";
import type { FileNode } from "../types";

function FileIcon({ kind }: { kind: FileNode["kind"] }) {
  if (kind === "dir") return <IconFolder />;
  if (kind === "image") return <IconImage />;
  return <IconFile />;
}

function TreeRows({
  nodes,
  depth,
  expanded,
  toggle,
}: {
  nodes: FileNode[];
  depth: number;
  expanded: Set<string>;
  toggle: (id: string) => void;
}) {
  const { state, dispatch } = useStore();
  const requestOpen = (path: string) => {
    if (state.openFileDirty && state.openFilePath && state.openFilePath !== path) {
      dispatch({
        type: "ask-confirm",
        confirm: {
          title: "未保存的修改",
          body: "当前文件有未保存修改。保存后再打开新文件？",
          confirmLabel: "保存并打开",
          onConfirm: () => {
            dispatch({ type: "save-file" });
            dispatch({ type: "open-file", path });
          },
          extraLabel: "放弃改动",
          onExtra: () => {
            dispatch({ type: "discard-draft" });
            dispatch({ type: "open-file", path });
          },
        },
      });
      return;
    }
    dispatch({ type: "open-file", path });
  };
  const sorted = useMemo(
    () => [...nodes].sort((a, b) => (a.kind === "dir" ? 0 : 1) - (b.kind === "dir" ? 0 : 1) || a.name.localeCompare(b.name, "zh")),
    [nodes],
  );
  return (
    <>
      {sorted.map((n) => {
        const open = expanded.has(n.id);
        return (
          <div key={n.id}>
            <button
              type="button"
              className={`tree-row${state.openFilePath === n.path ? " is-on" : ""}`}
              style={{ paddingLeft: `calc(var(--space-1) + var(--density) * ${depth * 2})` }}
              onClick={() => {
                if (n.kind === "dir") toggle(n.id);
                else requestOpen(n.path);
              }}
              onDoubleClick={() => {
                if (n.kind !== "dir") requestOpen(n.path);
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                dispatch({ type: "set-context-menu", menu: { x: e.clientX, y: e.clientY, filePath: n.path } });
              }}
            >
              {n.kind === "dir" ? (
                <IconChevron style={{ transform: open ? "rotate(0deg)" : "rotate(-90deg)", flex: "0 0 auto" }} />
              ) : (
                <span className="tree-indent" />
              )}
              <FileIcon kind={n.kind} />
              <span className="tree-name">{n.name}</span>
            </button>
            {n.kind === "dir" && open && n.children && (
              <TreeRows nodes={n.children} depth={depth + 1} expanded={expanded} toggle={toggle} />
            )}
          </div>
        );
      })}
    </>
  );
}

export function FileTree() {
  const { state, dispatch } = useStore();
  const [expanded, setExpanded] = useState<Set<string>>(new Set(["d-docs", "d-src"]));
  const roots = nodesAt(state.files, []);
  const crumbs = ["工作目录", ...state.cwd];

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", minWidth: 0 }}>
      <div className="addr">
        {crumbs.map((c, i) => (
          <button
            key={`${c}-${i}`}
            type="button"
            className={`crumb${i === 0 ? " is-root" : ""}`}
            onClick={() => dispatch({ type: "set-cwd", cwd: state.cwd.slice(0, i) })}
          >
            {i > 0 ? "/ " : ""}
            {c}
          </button>
        ))}
        <button
          type="button"
          className="icon-btn"
          style={{ marginLeft: "auto" }}
          aria-label="收起编辑区"
          title="收起编辑区"
          onClick={() => dispatch({ type: "toggle-editor" })}
        >
          <IconDrawer />
        </button>
      </div>
      <div
        className="tree"
        onContextMenu={(e) => {
          if ((e.target as HTMLElement).closest(".tree-row")) return;
          e.preventDefault();
          dispatch({ type: "set-context-menu", menu: { x: e.clientX, y: e.clientY, filePath: "" } });
        }}
      >
        {roots.length === 0 ? (
          <div className="empty">工作目录是空的 —— 右键新建文件夹或上传。</div>
        ) : (
          <TreeRows
            nodes={roots}
            depth={0}
            expanded={expanded}
            toggle={(id) => {
              setExpanded((prev) => {
                const n = new Set(prev);
                if (n.has(id)) n.delete(id);
                else n.add(id);
                return n;
              });
            }}
          />
        )}
      </div>
    </div>
  );
}
