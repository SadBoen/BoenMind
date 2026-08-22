import { IconDirTree, IconMenu } from "../lib/icons";
import { useStore } from "../store";

export function Topbar({
  title,
  showFiles,
}: {
  title: string;
  showFiles?: boolean;
}) {
  const { state, dispatch } = useStore();
  return (
    <header className="topbar">
      {state.view === "chat" && (
        <button
          type="button"
          className="icon-btn"
          aria-label="会话栏"
          title="会话栏"
          onClick={() => dispatch({ type: "toggle-session-collapsed" })}
        >
          <IconMenu />
        </button>
      )}
      <h1 className="unit-title">{title}</h1>
      {showFiles && (
        <button
          type="button"
          className="icon-btn"
          aria-label="文件"
          title="文件"
          onClick={() => dispatch({ type: "toggle-file-dock" })}
        >
          <IconDirTree />
        </button>
      )}
    </header>
  );
}
