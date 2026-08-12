/**
 * 最左侧 48px 竖排导航栏：由 lib/navigation.tsx 注册表驱动
 * （顶部 = 非 bottom 项，底部 = bottom 项；占位项禁用）。
 */
import { useTranslation } from "react-i18next";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { NAV, type NavKey } from "@/lib/navigation";
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
  const { t } = useTranslation();
  const activeNav = useAppStore((s) => s.activeNav);
  const setNav = useAppStore((s) => s.setNav);

  const entries = Object.entries(NAV);
  const top = entries.filter(([, e]) => !e.bottom);
  const bottom = entries.filter(([, e]) => e.bottom);

  return (
    <nav
      className="flex shrink-0 flex-col items-center border-r bg-muted/30 py-3"
      style={{ width: NAV_WIDTH }}
    >
      <div className="flex flex-1 flex-col items-center gap-2">
        {top.map(([key, entry]) => (
          <NavButton
            key={key}
            icon={entry.icon}
            label={t(entry.labelKey)}
            active={activeNav === key}
            disabled={entry.placeholder}
            onClick={() => setNav(key as NavKey)}
          />
        ))}
      </div>
      <div className="flex flex-col items-center gap-2">
        {bottom.map(([key, entry]) => (
          <NavButton
            key={key}
            icon={entry.icon}
            label={t(entry.labelKey)}
            active={activeNav === key}
            onClick={() => setNav(key as NavKey)}
          />
        ))}
      </div>
    </nav>
  );
}
