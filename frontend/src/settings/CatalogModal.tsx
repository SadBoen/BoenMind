import { Row, SettingsForm, Text, Toggle } from "../components/SettingsForm";
import { useStore } from "../store";
import { toast } from "../lib/toast";

export function CatalogModal() {
  const { state, dispatch } = useStore();
  const m = state.modal;
  if (!m) return null;
  const item = m.item;
  return (
    <div className="modal-center" role="dialog" aria-modal="true">
      <div className="dialog-card">
        <div style={{ display: "flex", alignItems: "center" }}>
          <h3 style={{ flex: 1 }}>{m.title}</h3>
          <button type="button" className="icon-btn" aria-label="关闭" onClick={() => dispatch({ type: "close-modal" })}>
            ×
          </button>
        </div>
        <SettingsForm>
          {Object.entries(item.config).map(([k, v]) =>
            typeof v === "boolean" ? (
              <Row key={k} label={k}>
                <Toggle
                  checked={v}
                  onChange={(nv) =>
                    dispatch({
                      type: "patch-catalog",
                      kind: m.kind,
                      id: item.id,
                      config: { ...item.config, [k]: nv },
                    })
                  }
                />
              </Row>
            ) : (
              <Row key={k} label={k}>
                <Text
                  value={String(v)}
                  onChange={(nv) =>
                    dispatch({
                      type: "patch-catalog",
                      kind: m.kind,
                      id: item.id,
                      config: { ...item.config, [k]: nv },
                    })
                  }
                />
              </Row>
            ),
          )}
        </SettingsForm>
        <div className="modal-actions">
          <button
            type="button"
            className="btn-solid"
            onClick={() => {
              toast.success("已保存");
              dispatch({ type: "close-modal" });
            }}
          >
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
