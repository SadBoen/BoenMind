import { useCallback, useEffect, useRef, useState } from "react";

// 限时提示 hook(收编各设置页散落的裸 setTimeout):重复触发清旧定时器,
// 组件卸载时统一清理,消除「卸载后 setState」内存泄漏告警。
export function useTimedNotice(timeoutMs = 4000) {
  const [notice, setNotice] = useState<string | null>(null);
  const timerRef = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (timerRef.current !== null) window.clearTimeout(timerRef.current);
    },
    [],
  );
  const clearNotice = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    setNotice(null);
  }, []);
  const flash = useCallback(
    (msg: string) => {
      if (timerRef.current !== null) window.clearTimeout(timerRef.current);
      setNotice(msg);
      timerRef.current = window.setTimeout(() => setNotice(null), timeoutMs);
    },
    [timeoutMs],
  );
  return { notice, flash, clearNotice, setNotice };
}
