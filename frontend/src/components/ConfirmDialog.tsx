import { useStore } from "../store";

export function ConfirmDialog() {
  const { state, dispatch } = useStore();
  const c = state.confirm;
  if (!c) return null;
  return (
    <div className="modal-center" role="dialog" aria-modal="true" aria-labelledby="confirm-title">
      <div className="dialog-card">
        <h3 id="confirm-title">{c.title}</h3>
        <p>{c.body}</p>
        <div className="modal-actions">
          <button type="button" className="btn-ghost" onClick={() => dispatch({ type: "close-confirm" })}>
            取消
          </button>
          {c.onExtra && (
            <button
              type="button"
              className="btn-ghost"
              onClick={() => {
                c.onExtra?.();
                dispatch({ type: "close-confirm" });
              }}
            >
              {c.extraLabel ?? "放弃"}
            </button>
          )}
          <button
            type="button"
            className={`btn-solid${c.danger ? " is-danger" : ""}`}
            onClick={() => {
              c.onConfirm();
              dispatch({ type: "close-confirm" });
            }}
          >
            {c.confirmLabel ?? "确认"}
          </button>
        </div>
      </div>
    </div>
  );
}
