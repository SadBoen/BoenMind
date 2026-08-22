import { Suspense, useState } from "react";
import { Topbar } from "../layouts/Topbar";
import { CatalogModal } from "../settings/CatalogModal";
import { getSections } from "../settings/registry";
import { saveSettings } from "../lib/storage";
import { toast } from "../lib/toast";
import { useStore } from "../store";

const SECTIONS = getSections();

export function SettingsView() {
  const { state } = useStore();
  const [id, setId] = useState(SECTIONS[0]?.id ?? "general");
  const current = SECTIONS.find((s) => s.id === id) ?? SECTIONS[0];
  const Comp = current.component;
  return (
    <div className="main-col">
      <Topbar title="设置" />
      <div className="settings">
        <nav className="settings-nav" aria-label="设置分区">
          <div className="settings-nav-list">
            {SECTIONS.map((s) => (
              <button key={s.id} type="button" className={s.id === id ? "is-on" : ""} onClick={() => setId(s.id)}>
                {s.icon} {s.label}
              </button>
            ))}
          </div>
          <button
            type="button"
            className="settings-save"
            onClick={() => {
              saveSettings(state.settings);
              toast.success("已保存");
            }}
          >
            保存
          </button>
        </nav>
        <div className="settings-body">
          <div className="settings-page">
            <Suspense fallback={<div className="sk" />}>{Comp ? <Comp /> : null}</Suspense>
          </div>
        </div>
      </div>
      <CatalogModal />
    </div>
  );
}
