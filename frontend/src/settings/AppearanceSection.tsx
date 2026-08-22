import { useRef } from "react";
import { Row, Select, SettingsForm } from "../components/SettingsForm";
import { useStore } from "../store";

export default function AppearanceSection() {
  const { state, dispatch } = useStore();
  const s = state.settings;
  const fileRef = useRef<HTMLInputElement>(null);

  const pickImage = (f: File | undefined) => {
    if (!f) return;
    const reader = new FileReader();
    reader.onload = () => {
      const url = String(reader.result ?? "");
      dispatch({ type: "patch-settings", patch: { bgUrl: url } });
    };
    reader.readAsDataURL(f);
  };

  return (
    <SettingsForm>
      <Row label="风格">
        <Select
          value={s.style}
          onChange={(v) => dispatch({ type: "patch-settings", patch: { style: v as "modern" | "cartoon" } })}
          options={[
            { value: "modern", label: "现代黑白" },
            { value: "cartoon", label: "卡通" },
          ]}
        />
      </Row>
      <Row label="材质">
        <Select
          value={s.material}
          onChange={(v) => dispatch({ type: "patch-settings", patch: { material: v as "solid" | "glass" } })}
          options={[
            { value: "solid", label: "纯色" },
            { value: "glass", label: "毛玻璃" },
          ]}
        />
      </Row>
      <Row label="背景类型">
        <Select
          value={s.background}
          onChange={(v) => dispatch({ type: "patch-settings", patch: { background: v as "solid" | "image" } })}
          options={[
            { value: "solid", label: "纯色" },
            { value: "image", label: "图片" },
          ]}
        />
      </Row>

      {s.background === "image" && (
        <>
          <Row label="背景图片">
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-1)", width: "100%" }}>
              <div style={{ display: "flex", gap: "var(--space-1)", alignItems: "center" }}>
                <button type="button" className="btn-ghost" onClick={() => fileRef.current?.click()}>
                  上传图片
                </button>
                <input
                  ref={fileRef}
                  type="file"
                  accept="image/*"
                  style={{ display: "none" }}
                  onChange={(e) => pickImage(e.target.files?.[0])}
                />
                <span style={{ color: "var(--fg-3)", fontSize: "var(--font-sm)" }}>或</span>
                <input
                  className="field"
                  placeholder="图片 URL"
                  value={s.bgUrl.startsWith("data:") ? "" : s.bgUrl}
                  onChange={(e) => dispatch({ type: "patch-settings", patch: { bgUrl: e.target.value } })}
                />
              </div>
              {s.bgUrl && (
                <div style={{ display: "flex", gap: "var(--space-1)", alignItems: "center" }}>
                  <img
                    src={s.bgUrl}
                    alt="背景预览"
                    style={{
                      width: "calc(var(--session-w))",
                      height: "calc(var(--font-body) * 4)",
                      objectFit: "cover",
                      borderRadius: "var(--radius-sm)",
                      border: "1px solid var(--stroke)",
                    }}
                  />
                  <span
                    style={{ fontSize: "var(--font-sm)", color: "var(--fg-3)" }}
                    title={s.bgUrl.startsWith("data:") ? "已用上传的图片" : s.bgUrl}
                  >
                    {s.bgUrl.startsWith("data:") ? "本地图片已预览" : "已载入"}
                  </span>
                </div>
              )}
            </div>
          </Row>
          <Row label="背景遮罩">
            <input
              className="field"
              type="range"
              min={0}
              max={100}
              value={Math.round(s.glassG * 100)}
              onChange={(e) => dispatch({ type: "patch-settings", patch: { glassG: Number(e.target.value) / 100 } })}
            />
            <span style={{ marginLeft: "var(--space-1)", color: "var(--fg-2)" }}>
              {Math.round(s.glassG * 100)}%
            </span>
          </Row>
        </>
      )}

      <Row label="推理显示">
        <Select
          value={s.thinkingDisplay}
          onChange={(v) => dispatch({ type: "patch-settings", patch: { thinkingDisplay: v as typeof s.thinkingDisplay } })}
          options={[
            { value: "auto", label: "auto（来推展开、完事收起）" },
            { value: "expanded", label: "expanded" },
            { value: "hidden", label: "hidden" },
          ]}
        />
      </Row>
      {s.material === "glass" && (
        <>
          <Row label="玻璃透明度">
            <input
              className="field"
              type="range"
              min={0}
              max={100}
              value={Math.round(s.glassG * 100)}
              onChange={(e) => dispatch({ type: "patch-settings", patch: { glassG: Number(e.target.value) / 100 } })}
            />
            <span style={{ marginLeft: "var(--space-1)", color: "var(--fg-2)" }}>{Math.round(s.glassG * 100)}%</span>
          </Row>
          <Row label="毛玻璃色调">
            <input
              className="field"
              type="range"
              min={0}
              max={40}
              value={s.glassHue}
              onChange={(e) => dispatch({ type: "patch-settings", patch: { glassHue: Number(e.target.value) } })}
            />
          </Row>
        </>
      )}
    </SettingsForm>
  );
}