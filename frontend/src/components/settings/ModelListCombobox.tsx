/**
 * 模型列表下拉（合并「模型文本框 + 默认模型下拉」）：
 * - 触发器显示默认模型 + 模型数
 * - 面板：每行 = 设为默认（星标）+ 名称（点击行内编辑）+ 删除；底部输入框添加新模型
 *
 * 自绘轻量 Popover（外部点击 / Escape 关闭），避免菜单组件与行内输入框的键盘冲突。
 */
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Plus, Star, X } from "lucide-react";
import { cn } from "@/lib/utils";

export function ModelListCombobox({
  models,
  defaultModel,
  onModelsChange,
  onDefaultChange,
}: {
  models: string[];
  defaultModel?: string;
  onModelsChange: (models: string[]) => void;
  onDefaultChange: (model: string | undefined) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const [adding, setAdding] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const editInputRef = useRef<HTMLInputElement>(null);
  const addInputRef = useRef<HTMLInputElement>(null);

  // 外部点击 / Escape 关闭
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  // 进入编辑行时聚焦并全选
  useEffect(() => {
    if (editingIndex !== null) {
      editInputRef.current?.focus();
      editInputRef.current?.select();
    } else {
      addInputRef.current?.focus();
    }
  }, [editingIndex]);

  const commitEdit = () => {
    if (editingIndex === null) return;
    const name = draft.trim();
    if (name && !models.some((m, i) => i !== editingIndex && m === name)) {
      const wasDefault = models[editingIndex] === defaultModel;
      const next = [...models];
      next[editingIndex] = name;
      onModelsChange(next);
      if (wasDefault) onDefaultChange(name);
    }
    setEditingIndex(null);
  };

  const removeModel = (index: number) => {
    const removed = models[index];
    const next = models.filter((_, i) => i !== index);
    onModelsChange(next);
    // 删掉默认模型时默认移到第一个
    if (removed === defaultModel) onDefaultChange(next[0]);
    if (editingIndex === index) setEditingIndex(null);
  };

  const addModel = () => {
    const name = adding.trim();
    if (!name || models.includes(name)) {
      setAdding("");
      return;
    }
    const next = [...models, name];
    onModelsChange(next);
    // 列表原本为空时新模型自动成为默认
    if (models.length === 0) onDefaultChange(name);
    setAdding("");
  };

  return (
    <div ref={rootRef} className="relative">
      {/* 触发器：默认模型 + 模型数 */}
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className={cn(
          "flex h-9 w-full items-center justify-between rounded-md border border-input bg-transparent px-3 text-sm shadow-xs transition-colors",
          "hover:bg-accent/40 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
          open && "bg-accent/40",
        )}
      >
        <span className="flex min-w-0 items-center gap-1.5">
          {models.length === 0 ? (
            <span className="text-muted-foreground">{t("settings.providers.noModels")}</span>
          ) : (
            <>
              <Star
                size={12}
                className={cn("shrink-0", defaultModel ? "fill-amber-400 text-amber-400" : "text-muted-foreground")}
              />
              <span className="truncate font-medium">{defaultModel ?? models[0]}</span>
              <span className="shrink-0 text-xs text-muted-foreground">
                · {t("settings.providers.pickerModels", { count: models.length })}
              </span>
            </>
          )}
        </span>
        <ChevronDown size={14} className={cn("shrink-0 text-muted-foreground transition-transform", open && "rotate-180")} />
      </button>

      {/* 面板 */}
      {open && (
        <div className="absolute left-0 top-full z-50 mt-1.5 w-full min-w-72 rounded-lg border bg-popover p-1.5 text-popover-foreground shadow-md ring-1 ring-foreground/10">
          {/* 模型列表 */}
          <div className="max-h-52 overflow-y-auto">
            {models.length === 0 && (
              <p className="px-2 py-3 text-center text-xs text-muted-foreground">
                {t("settings.providers.noModels")}
              </p>
            )}
            {models.map((m, i) => {
              const isDefault = m === defaultModel;
              if (editingIndex === i) {
                return (
                  <input
                    key={i}
                    ref={editInputRef}
                    value={draft}
                    onChange={(e) => setDraft(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") commitEdit();
                      if (e.key === "Escape") {
                        e.stopPropagation();
                        setEditingIndex(null);
                      }
                    }}
                    onBlur={commitEdit}
                    className="mb-0.5 h-7 w-full rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  />
                );
              }
              return (
                <div
                  key={i}
                  className="group flex h-7 items-center gap-1 rounded-md px-1.5 hover:bg-accent/50"
                >
                  <button
                    type="button"
                    title={isDefault ? t("settings.providers.currentDefault") : t("settings.providers.setDefault")}
                    onClick={() => onDefaultChange(isDefault ? undefined : m)}
                    className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-accent"
                  >
                    <Star
                      size={13}
                      className={cn(isDefault && "fill-amber-400 text-amber-400")}
                    />
                  </button>
                  <button
                    type="button"
                    title={t("settings.providers.editModel")}
                    onClick={() => {
                      setEditingIndex(i);
                      setDraft(m);
                    }}
                    className="min-w-0 flex-1 truncate rounded px-1 py-0.5 text-left text-xs hover:bg-accent"
                  >
                    {m}
                  </button>
                  <button
                    type="button"
                    title={t("settings.providers.deleteModel")}
                    onClick={() => removeModel(i)}
                    className="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 transition-opacity hover:bg-accent hover:text-destructive group-hover:opacity-100"
                  >
                    <X size={13} />
                  </button>
                </div>
              );
            })}
          </div>

          {/* 添加新模型 */}
          <div className="mt-1 flex items-center gap-1.5 border-t pt-1.5">
            <input
              ref={addInputRef}
              value={adding}
              onChange={(e) => setAdding(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") addModel();
                if (e.key === "Escape") e.stopPropagation();
              }}
              placeholder={t("settings.providers.addModelPlaceholder")}
              className="h-7 min-w-0 flex-1 rounded-md border border-input bg-background px-2 text-xs outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
            />
            <button
              type="button"
              title={t("settings.providers.addModel")}
              onClick={addModel}
              disabled={!adding.trim()}
              className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-input text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-40"
            >
              <Plus size={13} />
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
