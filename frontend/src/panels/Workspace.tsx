import { useEffect, useRef } from "react";
import { Panel, PanelGroup, PanelResizeHandle, type ImperativePanelHandle } from "react-resizable-panels";
import { IconCode, IconWiki } from "../lib/icons";
import { pxToPercent, useMinSizePercent } from "../lib/minSize";
import { useStore } from "../store";
import { Topbar } from "../layouts/Topbar";
import { ChatPanel } from "./ChatPanel";
import { FileDock } from "./FileDock";
import { PlaceholderView } from "./PlaceholderView";
import { SessionPanel } from "./SessionPanel";
import { SettingsView } from "./SettingsView";

export function Workspace() {
  const { state, dispatch } = useStore();
  const sessionRef = useRef<ImperativePanelHandle>(null);
  const fileRef = useRef<ImperativePanelHandle>(null);
  const sessionCmd = useRef(false);
  const fileCmd = useRef(false);
  const { hostRef, minSize } = useMinSizePercent(224);
  const chat = state.view === "chat";

  useEffect(() => {
    if (state.narrow) return;
    const p = sessionRef.current;
    if (!p) return;
    sessionCmd.current = true;
    if (state.sessionCollapsed) p.collapse();
    else p.expand();
  }, [state.sessionCollapsed, state.narrow]);

  useEffect(() => {
    if (state.narrow || !chat) return;
    const p = fileRef.current;
    const host = hostRef.current;
    if (!p) return;
    if (state.fileDockCollapsed) {
      fileCmd.current = true;
      p.collapse();
      return;
    }
    fileCmd.current = true;
    p.expand();
    const w = host?.getBoundingClientRect().width ?? 0;
    if (w <= 0) return;
    if (state.editorCollapsed) p.resize(pxToPercent(280, w));
    else p.resize(Math.max(28, pxToPercent(280, w) * 2));
  }, [state.fileDockCollapsed, state.editorCollapsed, state.narrow, chat]);

  const main =
    state.view === "chat" ? (
      <ChatPanel />
    ) : state.view === "settings" ? (
      <SettingsView />
    ) : state.view === "code" ? (
      <div className="main-col">
        <Topbar title="编程" />
        <PlaceholderView icon={<IconCode width={48} height={48} />} title="编程视图待实现" />
      </div>
    ) : (
      <div className="main-col">
        <Topbar title="WIKI" />
        <PlaceholderView icon={<IconWiki width={48} height={48} />} title="WIKI 视图待实现" />
      </div>
    );

  if (state.narrow) {
    return (
      <div className="workspace" ref={hostRef}>
        {main}
        {chat && !state.sessionCollapsed && (
          <>
            <button type="button" className="mask-overlay" aria-label="关闭会话栏" onClick={() => dispatch({ type: "set-session-collapsed", value: true })} />
            <aside className="slide-drawer is-left">
              <SessionPanel />
            </aside>
          </>
        )}
        {chat && !state.fileDockCollapsed && (
          <>
            <button type="button" className="mask-overlay" aria-label="关闭文件" onClick={() => dispatch({ type: "set-file-dock", value: true })} />
            <aside className="slide-drawer is-right">
              <FileDock />
            </aside>
          </>
        )}
      </div>
    );
  }

  return (
    <div className="workspace" ref={hostRef}>
      <PanelGroup direction="horizontal" autoSaveId="boenmind-workspace" style={{ height: "100%", width: "100%" }}>
        {chat && (
          <>
            <Panel
              id="session"
              ref={sessionRef}
              order={1}
              collapsible
              collapsedSize={0}
              minSize={minSize}
              defaultSize={22}
              onCollapse={() => {
                if (sessionCmd.current) {
                  sessionCmd.current = false;
                  if (!state.sessionCollapsed) dispatch({ type: "set-session-collapsed", value: true });
                  return;
                }
                requestAnimationFrame(() => sessionRef.current?.expand());
              }}
              onExpand={() => {
                sessionCmd.current = false;
                if (state.sessionCollapsed) dispatch({ type: "set-session-collapsed", value: false });
              }}
            >
              <SessionPanel />
            </Panel>
            <PanelResizeHandle className="resize-handle" />
          </>
        )}
        <Panel id="main" order={2} minSize={30} defaultSize={chat ? 54 : 100}>
          {main}
        </Panel>
        {chat && (
          <>
            <PanelResizeHandle className="resize-handle" />
            <Panel
              id="files"
              ref={fileRef}
              order={3}
              collapsible
              collapsedSize={0}
              minSize={minSize}
              defaultSize={24}
              onCollapse={() => {
                if (fileCmd.current) {
                  fileCmd.current = false;
                  if (!state.fileDockCollapsed) dispatch({ type: "set-file-dock", value: true });
                  return;
                }
                requestAnimationFrame(() => fileRef.current?.expand());
              }}
              onExpand={() => {
                fileCmd.current = false;
                if (state.fileDockCollapsed) dispatch({ type: "set-file-dock", value: false });
              }}
            >
              <FileDock />
            </Panel>
          </>
        )}
      </PanelGroup>
    </div>
  );
}
