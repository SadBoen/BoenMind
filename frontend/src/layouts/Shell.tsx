import { Toaster } from "sonner";
import { ApprovalDialog } from "../components/ApprovalDialog";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { ContextMenu } from "../components/ContextMenu";
import { Workspace } from "../panels/Workspace";
import { useStore } from "../store";
import { IconNav } from "./IconNav";
import { StatusBar } from "./StatusBar";

export function Shell() {
  const { state } = useStore();
  return (
    <div className={`app${state.streaming ? " is-live-root" : ""}`}>
      <IconNav />
      <div className="shell">
        <Workspace />
        <StatusBar />
      </div>
      <ApprovalDialog />
      <ConfirmDialog />
      <ContextMenu />
      <Toaster position="bottom-right" visibleToasts={3} closeButton />
    </div>
  );
}
