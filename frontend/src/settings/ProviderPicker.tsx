import { useEffect, useRef, useState } from "react";
import { IconSearch } from "../lib/icons";
import { GROUP_LABELS, KIND_GROUPS, KIND_LABELS, KIND_PRESETS } from "../lib/provider-presets";
import type { ProviderKind } from "../types";
import { ProviderIcon } from "./provider-icons";

export function ProviderPicker({
  open,
  onClose,
  onPick,
}: {
  open: boolean;
  onClose: () => void;
  onPick: (kind: ProviderKind) => void;
}) {
  const [search, setSearch] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setSearch("");
      const t = window.setTimeout(() => inputRef.current?.focus(), 30);
      return () => window.clearTimeout(t);
    }
  }, [open]);

  if (!open) return null;

  const q = search.trim().toLowerCase();
  const matches = (k: ProviderKind) => !q || KIND_LABELS[k].toLowerCase().includes(q) || k.includes(q);

  return (
    <div className="modal-center" role="dialog" aria-modal="true" aria-labelledby="picker-title">
      <div className="dialog-card is-wide">
        <div className="dialog-head">
          <h3 id="picker-title">添加提供商</h3>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={onClose}>
            ×
          </button>
        </div>
        <div className="picker-search">
          <IconSearch />
          <input
            ref={inputRef}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") onClose();
            }}
            placeholder="搜索提供商"
          />
        </div>
        <div className="picker-body">
          {KIND_GROUPS.map(({ group, kinds }) => {
            const visible = kinds.filter(matches);
            if (visible.length === 0) return null;
            return (
              <div key={group} className="picker-group">
                <div className="picker-group-title">{GROUP_LABELS[group]}</div>
                <div className="picker-grid">
                  {visible.map((k) => (
                    <button key={k} type="button" className="picker-card" onClick={() => onPick(k)}>
                      <span className="provider-mark">
                        <ProviderIcon kind={k} size={22} />
                      </span>
                      <span className="picker-card-text">
                        <b>{KIND_LABELS[k]}</b>
                        <span className="muted">{KIND_PRESETS[k].base_url || "官方端点"}</span>
                      </span>
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
          {KIND_GROUPS.every(({ kinds }) => kinds.filter(matches).length === 0) && (
            <div className="empty">没有匹配的提供商</div>
          )}
        </div>
      </div>
    </div>
  );
}
