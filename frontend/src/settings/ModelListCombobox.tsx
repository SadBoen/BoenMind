import { useEffect, useRef, useState } from "react";
import { IconChevron, IconClose, IconPlus, IconStar, IconStarFill } from "../lib/icons";

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
  const [open, setOpen] = useState(false);
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const [adding, setAdding] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const editInputRef = useRef<HTMLInputElement>(null);
  const addInputRef = useRef<HTMLInputElement>(null);

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

  useEffect(() => {
    if (editingIndex !== null) {
      editInputRef.current?.focus();
      editInputRef.current?.select();
    } else if (open) {
      addInputRef.current?.focus();
    }
  }, [editingIndex, open]);

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
    if (models.length === 0) onDefaultChange(name);
    setAdding("");
  };

  return (
    <div ref={rootRef} className="model-combo">
      <button type="button" className={`model-combo-trigger${open ? " is-open" : ""}`} onClick={() => setOpen((o) => !o)}>
        <span className="model-combo-label">
          {models.length === 0 ? (
            <span className="muted">还没有模型</span>
          ) : (
            <>
              {defaultModel ? <IconStarFill className="is-star-on" /> : <IconStar />}
              <b>{defaultModel ?? models[0]}</b>
              <span className="muted">· {models.length} 个模型</span>
            </>
          )}
        </span>
        <IconChevron />
      </button>
      {open && (
        <div className="model-combo-panel" role="listbox">
          <div className="model-combo-list">
            {models.length === 0 && <p className="muted model-combo-empty">还没有模型，在下方添加。</p>}
            {models.map((m, i) => {
              const isDefault = m === defaultModel;
              if (editingIndex === i) {
                return (
                  <input
                    key={i}
                    ref={editInputRef}
                    className="field"
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
                  />
                );
              }
              return (
                <div key={i} className="model-combo-row">
                  <button
                    type="button"
                    className="icon-btn"
                    title={isDefault ? "当前默认" : "设为默认"}
                    onClick={() => onDefaultChange(isDefault ? undefined : m)}
                  >
                    {isDefault ? <IconStarFill className="is-star-on" /> : <IconStar />}
                  </button>
                  <button
                    type="button"
                    className="model-combo-name"
                    title="编辑模型名"
                    onClick={() => {
                      setEditingIndex(i);
                      setDraft(m);
                    }}
                  >
                    {m}
                  </button>
                  <button type="button" className="icon-btn model-combo-del" title="删除模型" onClick={() => removeModel(i)}>
                    <IconClose />
                  </button>
                </div>
              );
            })}
          </div>
          <div className="model-combo-add">
            <input
              ref={addInputRef}
              className="field"
              value={adding}
              placeholder="添加模型 id"
              onChange={(e) => setAdding(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") addModel();
                if (e.key === "Escape") e.stopPropagation();
              }}
            />
            <button type="button" className="icon-btn" title="添加模型" disabled={!adding.trim()} onClick={addModel}>
              <IconPlus />
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
