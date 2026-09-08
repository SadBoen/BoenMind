//! 表单字段(自 PluginsPage.tsx 机械移入)。
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
export function FormField({
  label,
  value,
  onChange,
  placeholder,
  mono,
  password,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  mono?: boolean;
  password?: boolean;
}) {
  return (
    <div className="space-y-1">
      <Label className="text-[11px]">{label}</Label>
      <Input
        type={password ? "password" : "text"}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        className={`h-7 text-[12px] ${mono ? "font-mono" : ""}`}
      />
    </div>
  );
}
