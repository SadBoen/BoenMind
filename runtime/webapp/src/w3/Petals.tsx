// 玻璃主题专属:樱花花瓣飘落层(已认可样张 §4.1 的 React 承载)。
// 仅玻璃主题挂载(App 内 theme==="glass" 时渲染);12 片花瓣参数
// useMemo 固定,避免重渲染抖动;动画本体在 theme.css(.petal)。
import { useMemo } from "react";

export function Petals() {
  const petals = useMemo(
    () =>
      Array.from({ length: 12 }, (_, i) => ({
        x: `${(i * 83) % 100}%`,
        s: `${8 + ((i * 7) % 9)}px`,
        t: `${9 + ((i * 13) % 8)}s`,
        d: `${-((i * 17) % 12)}s`,
        sw: `${-40 + ((i * 29) % 80)}px`,
      })),
    [],
  );
  return (
    <div className="petals" aria-hidden>
      {petals.map((p, i) => (
        <span
          key={i}
          className="petal"
          style={
            {
              "--x": p.x,
              "--s": p.s,
              "--t": p.t,
              "--d": p.d,
              "--sw": p.sw,
            } as React.CSSProperties
          }
        />
      ))}
    </div>
  );
}
