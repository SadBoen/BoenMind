import { useEffect, useMemo, useState } from "react";
import { dateGroup, formatTime } from "../lib/format";
import { IconArchive, IconMore, IconPlus, IconSearch, IconTag, IconTrash } from "../lib/icons";
import { useStore } from "../store";
import { toast } from "../lib/toast";
import type { DateGroup } from "../lib/format";
import type { Session } from "../types";

const ORDER: DateGroup[] = ["今天", "昨天", "更早"];

export function SessionPanel() {
  const { state, dispatch, visibleSessions, allTags } = useStore();
  const [menuId, setMenuId] = useState<string | null>(null);
  const [tagEditId, setTagEditId] = useState<string | null>(null);

  // 菜单点外部关闭（菜单内部点击靠容器 stopPropagation 不冒泡到 window）。
  useEffect(() => {
    if (!menuId) return;
    const close = () => setMenuId(null);
    window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, [menuId]);

  const grouped = useMemo(() => {
    const map: Record<DateGroup, Session[]> = { 今天: [], 昨天: [], 更早: [] };
    for (const s of visibleSessions) map[dateGroup(s.updatedAt)].push(s);
    return map;
  }, [visibleSessions]);

  const emptyFilter = state.selectedTags.length > 0 && visibleSessions.length === 0;

  return (
    <section className="unit">
      <div className="unit-body">
      <div style={{ padding: "var(--space-1)", display: "flex", flexDirection: "column", gap: "var(--space-1)" }}>
        <button type="button" className="pill-btn" onClick={() => dispatch({ type: "new-session" })}>
          <IconPlus /> 新建会话
        </button>
        <div style={{ position: "relative" }}>
          <IconSearch style={{ position: "absolute", left: "var(--space-1)", top: "50%", transform: "translateY(-50%)", color: "var(--fg-3)" }} />
          <input
            className="field"
            style={{ paddingLeft: "var(--control-h)" }}
            placeholder="搜索会话"
            value={state.sessionSearch}
            onChange={(e) => dispatch({ type: "set-search", q: e.target.value })}
          />
        </div>
        {allTags.length > 0 && (
          <div style={{ display: "flex", flexWrap: "wrap", gap: "calc(var(--density) * 2)" }}>
            {allTags.map((t) => (
              <button
                key={t}
                type="button"
                className={`chip${state.selectedTags.includes(t) ? " is-on" : ""}`}
                onClick={() => dispatch({ type: "toggle-tag-filter", tag: t })}
              >
                {t}
              </button>
            ))}
          </div>
        )}
      </div>
      <div className="session-list">
        {emptyFilter && <div className="empty">没有带这个标签的会话。</div>}
        {!emptyFilter && visibleSessions.length === 0 && <div className="empty">还没有会话 —— 在左侧新建一个开始。</div>}
        {ORDER.map((g) =>
          grouped[g].length ? (
            <div key={g}>
              <div className="date-label">{g}</div>
              {grouped[g].map((s) => (
                <div key={s.id} style={{ position: "relative" }}>
                  <button
                    type="button"
                    className={`session-row${state.activeSessionId === s.id ? " is-on" : ""}`}
                    onClick={() => dispatch({ type: "select-session", id: s.id })}
                  >
                    <span className="session-title">{s.title}</span>
                    <span className="session-time">{formatTime(s.updatedAt)}</span>
                    <span className="session-preview">{s.preview}</span>
                  </button>
                  <button
                    type="button"
                    className="icon-btn session-more"
                    style={{ position: "absolute", right: 0, top: 0 }}
                    aria-label="会话菜单"
                    onClick={(e) => {
                      e.stopPropagation();
                      setMenuId(menuId === s.id ? null : s.id);
                      setTagEditId(null);
                    }}
                  >
                    <IconMore />
                  </button>
                  {menuId === s.id && (
                    <div
                      className="pop-menu"
                      style={{ right: "var(--space-1)", top: "var(--control-h)" }}
                      onClick={(e) => e.stopPropagation()}
                    >
                      <button
                        type="button"
                        className="menu-item"
                        onClick={() => {
                          const title = window.prompt("重命名会话", s.title);
                          if (title) {
                            dispatch({ type: "rename-session", id: s.id, title });
                            toast.success("已重命名");
                          }
                          setMenuId(null);
                        }}
                      >
                        重命名
                      </button>
                      <button
                        type="button"
                        className="menu-item"
                        onClick={() => {
                          setTagEditId(s.id);
                        }}
                      >
                        <IconTag /> 设置标签
                      </button>
                      <button
                        type="button"
                        className="menu-item"
                        onClick={() => {
                          dispatch({ type: "archive-session", id: s.id, archived: true });
                          toast.success("已归档");
                          setMenuId(null);
                        }}
                      >
                        <IconArchive /> 归档
                      </button>
                      <button
                        type="button"
                        className="menu-item is-danger"
                        onClick={() => {
                          dispatch({
                            type: "ask-confirm",
                            confirm: {
                              title: "删除会话",
                              body: `确认删除「${s.title}」？`,
                              confirmLabel: "删除",
                              danger: true,
                              onConfirm: () => {
                                dispatch({ type: "delete-session", id: s.id });
                                toast.success("已删除会话");
                              },
                            },
                          });
                          setMenuId(null);
                        }}
                      >
                        <IconTrash /> 删除
                      </button>
                      {tagEditId === s.id && (
                        <div style={{ padding: "var(--space-1)", display: "flex", flexDirection: "column", gap: "calc(var(--density)*2)" }}>
                          {allTags.map((t) => {
                            const on = s.tags.includes(t);
                            return (
                              <button
                                key={t}
                                type="button"
                                className={`chip${on ? " is-on" : ""}`}
                                onClick={() => {
                                  const tags = on ? s.tags.filter((x) => x !== t) : [...s.tags, t];
                                  dispatch({ type: "set-session-tags", id: s.id, tags });
                                }}
                              >
                                {t}
                              </button>
                            );
                          })}
                          <button
                            type="button"
                            className="menu-item"
                            onClick={() => {
                              const t = window.prompt("新标签");
                              if (t) dispatch({ type: "set-session-tags", id: s.id, tags: [...s.tags, t] });
                            }}
                          >
                            + 新标签
                          </button>
                        </div>
                      )}
                    </div>
                  )}
                </div>
              ))}
            </div>
          ) : null,
        )}
      </div>
      </div>
    </section>
  );
}
