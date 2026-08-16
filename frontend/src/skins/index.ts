/**
 * 皮肤注册表（2026-08-16 皮肤系统）：一个皮肤一个文件夹（src/skins/<id>/），
 * style.css 由 index.css 顶部 @import 引入，作用域包在 :root[data-skin="<id>"] 下，
 * 切换 = html 挂 data-skin 属性，去掉即完整还原（借鉴 dsh-client-ui-aqua 的可逆性设计）。
 *
 * 皮肤只覆盖 CSS 令牌变量（shadcn 组件全部走令牌，零组件改动），
 * 可调参数经 --skin-<key> CSS 变量注入（lib/skin.ts 写入）。
 */
export interface SkinParam {
  /** 参数键：写入 --skin-<key> CSS 变量 */
  key: string;
  /** i18n key（settings.appearance.skin.param.<key>） */
  labelKey: string;
  min: number;
  max: number;
  step: number;
  default: number;
  /** 展示单位后缀 */
  format: "percent" | "px" | "deg";
}

export interface Skin {
  id: string;
  nameKey: string;
  descKey: string;
  params: SkinParam[];
}

export const SKINS = [
  {
    id: "classic",
    nameKey: "settings.appearance.skin.classic",
    descKey: "settings.appearance.skin.classicDesc",
    params: [],
  },
  {
    id: "glass",
    nameKey: "settings.appearance.skin.glass",
    descKey: "settings.appearance.skin.glassDesc",
    params: [
      {
        key: "hue",
        labelKey: "settings.appearance.skin.param.tint",
        min: 0,
        max: 360,
        step: 1,
        default: 250,
        format: "deg",
      },
      {
        key: "alpha",
        labelKey: "settings.appearance.skin.param.alpha",
        min: 30,
        max: 90,
        step: 5,
        default: 60,
        format: "percent",
      },
      {
        key: "blur",
        labelKey: "settings.appearance.skin.param.blur",
        min: 0,
        max: 24,
        step: 1,
        default: 16,
        format: "px",
      },
    ],
  },
] as const satisfies readonly Skin[];

export type SkinId = (typeof SKINS)[number]["id"];

/** 皮肤参数（key → 数值）；未设置的参数走 SkinParam.default */
export type SkinParams = Record<string, number>;
