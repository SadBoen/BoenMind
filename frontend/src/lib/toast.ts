import { toast as sonner } from "sonner";

const last = new Map<string, number>();

function dedup(key: string): boolean {
  const now = Date.now();
  const prev = last.get(key) ?? 0;
  if (now - prev < 1000) return false;
  last.set(key, now);
  return true;
}

export const toast = {
  success(msg: string) {
    if (!dedup(`s:${msg}`)) return;
    sonner.success(msg, { duration: 4000 });
  },
  info(msg: string) {
    if (!dedup(`i:${msg}`)) return;
    sonner(msg, { duration: 4000 });
  },
  error(msg: string) {
    if (!dedup(`e:${msg}`)) return;
    sonner.error(msg, { duration: 6000 });
  },
  loading(msg: string) {
    return sonner.loading(msg, { duration: Infinity });
  },
  dismiss(id?: string | number) {
    sonner.dismiss(id);
  },
};
