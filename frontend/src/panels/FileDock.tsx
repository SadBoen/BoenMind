import { Panel, PanelGroup, PanelResizeHandle, type ImperativePanelHandle } from "react-resizable-panels";
import { useEffect, useRef } from "react";
import { useMinSizePercent } from "../lib/minSize";
import { useStore } from "../store";
import { FileEditor } from "./FileEditor";
import { FileTree } from "./FileTree";

export function FileDock() {
  const { state, dispatch } = useStore();
  const editorRef = useRef<ImperativePanelHandle>(null);
  const cmd = useRef(false);
  const { hostRef, minSize } = useMinSizePercent(224);

  useEffect(() => {
    const p = editorRef.current;
    if (!p) return;
    cmd.current = true;
    if (state.editorCollapsed) p.collapse();
    else p.expand();
  }, [state.editorCollapsed]);

  return (
    <div className="file-dock" ref={hostRef}>
      <PanelGroup direction="horizontal" autoSaveId="boenmind-file-dock" style={{ height: "100%", width: "100%" }}>
        <Panel defaultSize={42} minSize={minSize} style={{ minWidth: 0 }}>
          <FileTree />
        </Panel>
        <PanelResizeHandle className="resize-handle" />
        <Panel
          ref={editorRef}
          collapsible
          collapsedSize={0}
          minSize={minSize}
          defaultSize={58}
          onCollapse={() => {
            if (cmd.current) {
              cmd.current = false;
              if (!state.editorCollapsed) dispatch({ type: "toggle-editor" });
              return;
            }
            requestAnimationFrame(() => editorRef.current?.expand());
          }}
          onExpand={() => {
            cmd.current = false;
            if (state.editorCollapsed) dispatch({ type: "toggle-editor" });
          }}
          style={{ minWidth: 0, maxWidth: "var(--editor-w)" }}
        >
          <FileEditor />
        </Panel>
      </PanelGroup>
    </div>
  );
}
