import { Topbar } from "../layouts/Topbar";
import { useStore } from "../store";
import { Composer } from "./Composer";
import { MessageList } from "./MessageList";

export function ChatPanel() {
  const { state } = useStore();
  const session = state.sessions.find((s) => s.id === state.activeSessionId);

  return (
    <div className="main-col">
      <Topbar title={session?.title ?? "聊天"} showFiles />
      <MessageList />
      <Composer />
    </div>
  );
}