/**
 * 添加提供商选择器：分组卡片 + 搜索过滤，选中即带入预设（参照 pi-web AddProviderPicker）。
 */
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Search } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { ProviderKind } from "@/api/client";
import { KIND_GROUPS, KIND_PRESETS } from "@/lib/provider-presets";
import { ProviderIcon } from "./provider-icons";

export function ProviderPicker({
  open,
  onOpenChange,
  onPick,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (kind: ProviderKind) => void;
}) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setSearch("");
      setTimeout(() => inputRef.current?.focus(), 30);
    }
  }, [open]);

  const q = search.trim().toLowerCase();
  const matches = (k: ProviderKind) =>
    !q || t(`settings.providers.kinds.${k}`).toLowerCase().includes(q) || k.includes(q);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("settings.providers.pickerTitle")}</DialogTitle>
        </DialogHeader>

        {/* 搜索 */}
        <div className="flex items-center gap-2 rounded-lg border bg-muted/40 px-3">
          <Search size={14} className="shrink-0 text-muted-foreground" />
          <input
            ref={inputRef}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") onOpenChange(false);
            }}
            placeholder={t("settings.providers.pickerSearch")}
            className="h-9 flex-1 border-none bg-transparent text-sm outline-none placeholder:text-muted-foreground"
          />
        </div>

        <div className="max-h-[55vh] overflow-y-auto pr-1">
          {KIND_GROUPS.map(({ group, kinds }) => {
            const visible = kinds.filter(matches);
            if (visible.length === 0) return null;
            return (
              <div key={group} className="mb-4 last:mb-0">
                <div className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
                  {t(`settings.providers.picker.${group}`)}
                </div>
                <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
                  {visible.map((k) => (
                    <button
                      key={k}
                      type="button"
                      onClick={() => onPick(k)}
                      className="flex items-center gap-2.5 rounded-lg border bg-card p-3 text-left transition-colors hover:bg-accent/60"
                    >
                      <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-muted/60">
                        <ProviderIcon kind={k} size={22} />
                      </div>
                      <div className="min-w-0">
                        <div className="truncate text-sm font-medium">
                          {t(`settings.providers.kinds.${k}`)}
                        </div>
                        <div className="truncate text-[10px] text-muted-foreground">
                          {KIND_PRESETS[k].base_url ||
                            t("settings.providers.officialEndpoint")}
                        </div>
                      </div>
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
          {KIND_GROUPS.every(({ kinds }) => kinds.filter(matches).length === 0) && (
            <div className="py-10 text-center text-sm text-muted-foreground">
              {t("settings.providers.pickerEmpty")}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
