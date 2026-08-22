import type { ReactNode } from "react";

export function PlaceholderView({ icon, title }: { icon: ReactNode; title: string }) {
  return (
    <div className="placeholder-view">
      <div>
        {icon}
        <strong>{title}</strong>
      </div>
    </div>
  );
}
