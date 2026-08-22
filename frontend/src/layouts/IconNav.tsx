import { IconChat, IconCode, IconGear, IconWiki } from "../lib/icons";
import { useStore } from "../store";
import type { ViewId } from "../types";

const ITEMS: { id: ViewId; label: string; icon: typeof IconChat }[] = [
  { id: "chat", label: "聊天", icon: IconChat },
  { id: "code", label: "编程", icon: IconCode },
  { id: "wiki", label: "WIKI", icon: IconWiki },
];

export function IconNav() {
  const { state, dispatch } = useStore();
  return (
    <nav className="rail" aria-label="主导航">
      <div className="rail-top">
        {ITEMS.map((it) => {
          const Icon = it.icon;
          const active = state.view === it.id;
          return (
            <button
              key={it.id}
              type="button"
              className={`rail-btn${active ? " is-active" : ""}${active && state.streaming ? " is-live" : ""}`}
              aria-label={it.label}
              aria-current={active ? "page" : undefined}
              title={it.label}
              onClick={() => dispatch({ type: "set-view", view: it.id })}
            >
              <Icon />
            </button>
          );
        })}
      </div>
      <button
        type="button"
        className={`rail-btn${state.view === "settings" ? " is-active" : ""}`}
        aria-label="设置"
        title="设置"
        onClick={() => dispatch({ type: "toggle-settings" })}
      >
        <IconGear />
      </button>
    </nav>
  );
}
