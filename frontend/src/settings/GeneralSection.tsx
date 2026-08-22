import { Row, Select, SettingsForm } from "../components/SettingsForm";
import { useStore } from "../store";

export default function GeneralSection() {
  const { state, dispatch } = useStore();
  return (
    <SettingsForm>
      <Row label="语言">
        <Select
          value={state.settings.language}
          onChange={(v) => dispatch({ type: "patch-settings", patch: { language: v as "zh" | "en" } })}
          options={[
            { value: "zh", label: "中文" },
            { value: "en", label: "English" },
          ]}
        />
      </Row>
      <Row label="界面字号">
        <Select
          value={state.settings.fontSize}
          onChange={(v) => dispatch({ type: "patch-settings", patch: { fontSize: v as "sm" | "md" | "lg" } })}
          options={[
            { value: "sm", label: "小" },
            { value: "md", label: "中" },
            { value: "lg", label: "大" },
          ]}
        />
      </Row>
    </SettingsForm>
  );
}
