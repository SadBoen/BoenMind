import type { FrostedKnobs } from './knobs.ts'

/** Official ThemeRuntime override layer: token → { light, dark }. */
export type ThemeTokenModes = { light: string; dark: string }
export type ThemeTokenOverrides = Record<string, ThemeTokenModes>

const pair = (light: string, dark: string): ThemeTokenModes => ({ light, dark })

const rgba = (rgb: string, alpha: number): string =>
  `rgba(${rgb}, ${Number(alpha.toFixed(3))})`

/**
 * Build a reversible override layer that turns official opaque fills into
 * frosted plates. Both palettes are always supplied so a scheme switch
 * cannot leave a token illegible (ThemeRuntime contract).
 * @param knobs - current glass opacity.
 */
export function glassTokenOverrides(knobs: FrostedKnobs): ThemeTokenOverrides {
  const a = knobs.glassOpacity
  const aBase = Math.max(0.08, a * 0.42)
  const aRaised = Math.min(0.92, a + 0.1)
  const aOverlay = Math.min(0.94, a + 0.22)
  const aInput = Math.min(0.9, a + 0.12)
  const aMenu = Math.min(0.9, a + 0.16)
  const aBubble = Math.min(0.88, a + 0.08)
  const aHover = Math.min(0.55, a * 0.7)

  return {
    '--dsw-alias-bg-base': pair(rgba('255, 255, 255', aBase), rgba('15, 17, 21', aBase)),
    '--dsw-alias-bg-layer-1': pair(rgba('255, 255, 255', a), rgba('27, 27, 28', a)),
    '--dsw-alias-bg-layer-2': pair(rgba('255, 255, 255', aRaised), rgba('33, 33, 35', aRaised)),
    '--dsw-alias-bg-layer-3': pair(rgba('248, 250, 252', aRaised), rgba('41, 41, 41', aRaised)),
    '--dsw-alias-bg-overlay': pair(rgba('255, 255, 255', aOverlay), rgba('44, 44, 46', aOverlay)),
    '--dsw-alias-bg-module-platform': pair(rgba('245, 246, 247', aRaised), rgba('53, 54, 56', aRaised)),
    '--dsw-specific-sidebar-fill': pair(rgba('249, 250, 251', a), rgba('21, 21, 23', a)),
    '--dsw-specific-input-major': pair(rgba('255, 255, 255', aInput), rgba('33, 33, 35', aInput)),
    '--dsw-specific-menu': pair(rgba('255, 255, 255', aMenu), rgba('41, 41, 41', aMenu)),
    '--dsw-specific-bubble': pair(rgba('237, 243, 254', aBubble), rgba('33, 33, 35', aBubble)),
    '--dsw-specific-selector': pair(rgba('245, 246, 247', aRaised), rgba('53, 54, 56', aRaised)),
    '--dsw-alias-button-elevated-fill': pair(rgba('255, 255, 255', aInput), rgba('67, 69, 74', aInput)),
    '--dsw-alias-button-floating-fill': pair(rgba('255, 255, 255', aInput), rgba('33, 33, 35', aInput)),
    '--dsw-alias-markdown-code-block': pair(rgba('250, 250, 251', aRaised), rgba('15, 15, 15', aRaised)),
    '--dsw-alias-markdown-inline-code': pair(rgba('245, 246, 247', aRaised), rgba('33, 33, 35', aRaised)),
    '--dsw-specific-sidebar-nav-item-active': pair(rgba('235, 238, 242', aRaised), rgba('67, 69, 74', aRaised)),
    '--dsw-specific-sidebar-nav-item-hover': pair(rgba('241, 243, 245', aHover), rgba('33, 33, 35', aHover)),
    '--dsw-alias-bg-mask-drop': pair(rgba('255, 255, 255', 0.45), rgba('15, 17, 21', 0.45)),
  }
}
