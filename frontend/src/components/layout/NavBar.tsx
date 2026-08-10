/**
 * 最左侧 48px 竖排导航栏：对话（激活）、图库/知识库（占位）、底部设置。
 */
import { BookOpenText, Images, MessageSquare, Settings } from "lucide-react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/stores/app-store";

const NAV_WIDTH = 48;

function NavButton({
  icon,
  label,
  active,
  disabled,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  active?: boolean;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        type="button"
        disabled={disabled}
        onClick={onClick}
        className={cn(
          "relative flex h-10 w-10 items-center justify-center rounded-lg transition-colors",
          active
            ? "bg-primary/10 text-primary"
            : "text-muted-foreground hover:bg-accent hover:text-foreground",
          disabled && "cursor-not-allowed opacity-40 hover:bg-transparent hover:text-muted-foreground",
        )}
      >
        {icon}
        {active && (
          <span className="absolute left-0 top-1/2 h-5 w-0.5 -translate-y-1/2 rounded-r bg-primary" />
        )}
      </TooltipTrigger>
      <TooltipContent side="right">{label}</TooltipContent>
    </Tooltip>
  );
}

export function NavBar() {
  const activeNav = useAppStore((s) => s.activeNav);
  const setNav = useAppStore((s) => s.setNav);

  return (
    <nav
      className="flex shrink-0 flex-col items-center border-r bg-muted/30 py-3"
      style={{ width: NAV_WIDTH }}
    >
      <div className="flex flex-1 flex-col items-center gap-2">
        <NavButton
          icon={<MessageSquare size={20} />}
          label="对话"
          active={activeNav === "chat"}
          onClick={() => setNav("chat")}
        />
        <NavButton icon={<Images size={20} />} label="图库（即将推出）" disabled />
        <NavButton icon={<BookOpenText size={20} />} label="知识库（即将推出）" disabled />
      </div>
      <div className="flex flex-col items-center gap-2">
        <NavButton
          icon={<Settings size={20} />}
          label="设置"
          active={activeNav === "settings"}
          onClick={() => setNav("settings")}
        />
      </div>
    </nav>
  );
}
