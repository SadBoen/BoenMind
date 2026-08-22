import { useMemo, useState } from "react";
import { IconGear, IconUninstall } from "../lib/icons";
import { useStore } from "../store";
import { toast } from "../lib/toast";
import type { CatalogItem } from "../types";

export function CatalogTable({ kind, items, emptyLabel }: { kind: "skill" | "plugin"; items: CatalogItem[]; emptyLabel: string }) {
  const { dispatch } = useStore();
  const [q, setQ] = useState("");
  const filtered = useMemo(() => items.filter((i) => i.name.toLowerCase().includes(q.trim().toLowerCase())), [items, q]);

  return (
    <div>
      <input className="field" placeholder="搜索名称" value={q} onChange={(e) => setQ(e.target.value)} style={{ marginBottom: "var(--space-1)" }} />
      {filtered.length === 0 ? (
        <div className="empty">{emptyLabel}</div>
      ) : (
        <table className="catalog-table">
          <thead>
            <tr>
              <th>名称</th>
              <th>类型</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((it) => (
              <tr key={it.id}>
                <td>{it.name}</td>
                <td>{it.type}</td>
                <td>
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label="设置"
                    onClick={() => dispatch({ type: "open-modal", modal: { title: `${it.name} 设置`, item: it, kind } })}
                  >
                    <IconGear />
                  </button>
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label="卸载"
                    disabled={it.builtin}
                    onClick={() =>
                      dispatch({
                        type: "ask-confirm",
                        confirm: {
                          title: "确认卸载",
                          body: `确认卸载 ${it.name}？`,
                          confirmLabel: "卸载",
                          danger: true,
                          onConfirm: () => {
                            dispatch({ type: "uninstall", kind, id: it.id });
                            toast.success(`已卸载 ${it.name}`);
                          },
                        },
                      })
                    }
                  >
                    <IconUninstall />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
