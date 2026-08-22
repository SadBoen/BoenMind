import type { ProviderKind } from "../types";

function FallbackIcon({ kind, size }: { kind: string; size: number }) {
  const label =
    kind
      .split(/[-_]/)
      .filter(Boolean)
      .slice(0, 2)
      .map((part) => part[0])
      .join("")
      .toUpperCase() || "?";
  return (
    <span
      aria-hidden="true"
      className="provider-fallback"
      style={{
        width: size,
        height: size,
        fontSize: Math.max(8, Math.floor(size * 0.42)),
      }}
    >
      {label}
    </span>
  );
}

function MiniMaxMark({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
      <rect width="24" height="24" rx="6" fill="#E11D48" />
      <path d="M6 16V8l4 6 4-6v8" fill="none" stroke="#fff" strokeWidth="1.8" strokeLinejoin="round" />
    </svg>
  );
}

function DeepSeekMark({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
      <rect width="24" height="24" rx="6" fill="#4D6BFE" />
      <path d="M7 16c2.4-6 8-8 10-9-1 3-1.2 6.2-4 8.2C11 16.6 8.6 16.4 7 16z" fill="#fff" />
    </svg>
  );
}

export function ProviderIcon({
  kind,
  size,
  label,
}: {
  kind: ProviderKind | string;
  size: number;
  label?: string;
}) {
  if (kind === "minimax") return <MiniMaxMark size={size} />;
  if (kind === "deepseek") return <DeepSeekMark size={size} />;
  return <FallbackIcon kind={label ?? kind} size={size} />;
}
