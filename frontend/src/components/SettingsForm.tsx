import type { ReactNode } from "react";

export function SettingsForm({ children }: { children: ReactNode }) {
  return <div className="settings-form">{children}</div>;
}

export function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="form-row">
      <label>{label}</label>
      <div>{children}</div>
    </div>
  );
}

export function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label?: string }) {
  return (
    <button type="button" className={`toggle${checked ? " is-on" : ""}`} role="switch" aria-checked={checked} aria-label={label} onClick={() => onChange(!checked)}>
      <i />
    </button>
  );
}

export function Select({
  value,
  onChange,
  options,
}: {
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
}) {
  return (
    <select className="field" value={value} onChange={(e) => onChange(e.target.value)}>
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export function Text({ value, onChange, type = "text", placeholder }: { value: string; onChange: (v: string) => void; type?: string; placeholder?: string }) {
  return <input className="field" type={type} value={value} placeholder={placeholder} onChange={(e) => onChange(e.target.value)} />;
}

export function Password({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return <Text value={value} onChange={onChange} type="password" />;
}
