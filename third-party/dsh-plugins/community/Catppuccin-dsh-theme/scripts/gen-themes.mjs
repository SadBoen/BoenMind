// Generates themes/catppuccin-<flavor>.json and lib/client.js from
// palette/palette.json. Run from the repo root: node scripts/gen-themes.mjs
import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const palette = JSON.parse(readFileSync(join(root, 'palette', 'palette.json'), 'utf8'))

const hexToRgb = (hex) => {
  const value = hex.replace('#', '')
  return [
    parseInt(value.slice(0, 2), 16),
    parseInt(value.slice(2, 4), 16),
    parseInt(value.slice(4, 6), 16),
  ]
}

const rgba = (hex, alpha) => `rgba(${hexToRgb(hex).join(', ')}, ${alpha})`

const mixHex = (a, b, t) => {
  const pa = hexToRgb(a)
  const pb = hexToRgb(b)
  const c = pa.map((v, i) => Math.round(v + (pb[i] - v) * t))
  return `#${c.map((v) => v.toString(16).padStart(2, '0')).join('')}`
}

// Catppuccin color -> --dsw-alias-* token mapping. Entries are either a color
// name or { color, alpha } for translucent roles. light covers latte; dark
// covers frappe/macchiato/mocha (surface order flips between the two). Token
// roles follow the official design-platform.css alias directory.
const TOKEN_MAPS = {
  dark: {
    // blue-450/500 stay pinned to mauve (the session meter "messages" chip
    // and trajectory labels use them); the rest of the static ladder is
    // generated from STATIC_LADDERS below. Everything else maps by alias.
    '--dsw-static-blue-450': 'mauve',
    '--dsw-static-blue-500': 'mauve',
    '--dsw-ctp-sky': 'sky',
    '--dsw-ctp-peach': 'peach',
    '--dsw-ctp-lavender': 'lavender',
    '--dsw-ctp-blue': 'blue',
    '--dsw-ctp-pink': 'pink',
    '--dsw-ctp-teal': 'teal',
    '--dsw-ctp-sapphire': 'sapphire',
    '--dsw-ctp-rosewater': 'rosewater',
    '--dsw-ctp-flamingo': 'flamingo',
    '--dsw-ctp-maroon': 'maroon',
    '--dsw-alias-bg-base': 'base',
    '--dsw-alias-bg-layer-1': 'mantle',
    '--dsw-alias-bg-layer-2': 'surface0',
    '--dsw-alias-bg-layer-3': 'surface1',
    '--dsw-alias-bg-overlay': 'surface0',
    '--dsw-alias-bg-mask-1': { color: 'crust', alpha: 0.5 },
    '--dsw-alias-bg-mask-2': { color: 'crust', alpha: 0.2 },
    '--dsw-alias-bg-mask-3': { color: 'crust', alpha: 0.48 },
    '--dsw-alias-bg-module-platform': 'surface0',
    '--dsw-alias-bg-multi-select': 'surface0',
    '--dsw-alias-bg-skeleton': { color: 'surface1', alpha: 0.08 },
    '--dsw-alias-border-l1': { color: 'overlay0', alpha: 0.25 },
    '--dsw-alias-border-l2': { color: 'overlay1', alpha: 0.45 },
    '--dsw-alias-border-l3': { color: 'overlay1', alpha: 0.55 },
    '--dsw-alias-border-l4': { color: 'overlay1', alpha: 0.7 },
    '--dsw-alias-label-primary': 'text',
    '--dsw-alias-label-secondary': 'subtext0',
    '--dsw-alias-label-tertiary': 'subtext1',
    '--dsw-alias-label-caption': 'subtext1',
    '--dsw-alias-label-dimmed': 'subtext1',
    '--dsw-alias-brand-primary': 'mauve',
    '--dsw-alias-brand-text': 'crust',
    '--dsw-alias-button-primary-hover': 'lavender',
    '--dsw-alias-button-primary-dimmed': 'surface0',
    '--dsw-alias-button-elevated-fill': 'surface0',
    '--dsw-alias-button-floating-fill': 'surface1',
    '--dsw-alias-button-floating-hover': 'surface2',
    '--dsw-alias-button-ghost-active-border': 'surface2',
    '--dsw-alias-button-ghost-active-fill': 'surface0',
    '--dsw-alias-button-ghost-active-hover': 'surface1',
    '--dsw-alias-state-business-primary': 'mauve',
    '--dsw-alias-state-business-tertiary': 'surface0',
    '--dsw-alias-state-error-primary': 'red',
    '--dsw-alias-state-error-secondary': 'red',
    '--dsw-alias-state-success-primary': 'green',
    '--dsw-alias-state-success-secondary': 'green',
    '--dsw-alias-state-success-tertiary': 'surface0',
    '--dsw-alias-state-warn-label': 'yellow',
    '--dsw-alias-state-warn-primary': 'yellow',
    '--dsw-alias-state-warn-secondary': 'yellow',
    '--dsw-alias-state-warn-tertiary': 'surface0',
    '--dsw-alias-interactive-bg-hover': { color: 'surface0', alpha: 0.45 },
    '--dsw-alias-interactive-bg-active': { color: 'surface1', alpha: 0.55 },
    '--dsw-alias-interactive-bg-hover-accent': { color: 'mauve', alpha: 0.14 },
    '--dsw-alias-interactive-bg-hover-danger': { color: 'red', alpha: 0.15 },
    '--dsw-alias-interactive-bg-hover-solid': 'surface1',
    // code block surface one step below the page background, exactly
    // like catppuccin-vscode's textCodeBlock.background (= mantle)
    '--dsw-alias-markdown-code-block': 'mantle',
    '--dsw-alias-markdown-code-block-banner': 'surface0',
    '--dsw-alias-markdown-code-segment-selected': 'surface0',
    '--dsw-alias-markdown-code-segment-unselected': 'mantle',
    '--dsw-alias-markdown-citation': 'surface0',
    '--dsw-alias-markdown-inline-code': 'surface0',
    '--dsw-alias-markdown-placeholder': 'surface0',
    '--dsw-alias-markdown-tag': 'surface0',
    '--dsw-alias-toast-bg': 'mantle',
    '--dsw-alias-tooltip-bg': 'surface0',
    '--dsw-specific-sidebar-fill': 'mantle',
    '--dsw-specific-sidebar-nav-item-active': 'surface1',
    '--dsw-specific-sidebar-nav-item-active-accent': { color: 'mauve', alpha: 0.25 },
    '--dsw-specific-sidebar-nav-item-hover': 'surface0',
    '--dsw-specific-bubble': 'surface0',
    '--dsw-specific-bubble-highlight': 'surface1',
    '--dsw-specific-input-major': 'mantle',
    '--dsw-specific-login-input': 'mantle',
    '--dsw-specific-menu': 'surface0',
    '--dsw-specific-selector': 'surface1',
    '--dsw-specific-tip': 'surface0',
    '--dsw-alias-separator-primary': { color: 'mauve', alpha: 0.8 },
    '--dsw-alias-scrollbar-bg-l1': 'surface0',
    '--dsw-alias-scrollbar-bg-l2': 'surface1',
    '--dsw-alias-scrollbar-hover-l1': 'surface2',
    '--dsw-alias-scrollbar-hover-l2': 'surface2',
    '--dsw-alias-bg-mask-photo': 'rgba(0, 0, 0, 0.88)',
    '--dsw-alias-bg-mask-drop': 'rgba(39, 39, 48, 0.7)',
    '--dsw-alias-border-inverted': 'rgba(255, 255, 255, 0.06)',
    '--dsw-alias-border-inverted2': 'rgba(255, 255, 255, 0.08)',
    '--dsw-alias-border-l2-darkmode-thin': { color: 'overlay1', alpha: 0.3 },
    '--dsw-alias-brand-primary-invert': 'text',
    '--dsw-alias-brand-primary-new-colorprimary-new-color': 'mauve',
    '--dsw-alias-button-contrast-fill': 'text',
    '--dsw-alias-button-info-fill': 'mauve',
    '--dsw-alias-button-info-hover': { mix: ['mauve', 'base'], t: 0.6 },
    '--dsw-alias-button-primary-fill': 'mauve',
    '--dsw-alias-button-tool-bar-fill': { color: 'overlay0', alpha: 0.5 },
    '--dsw-alias-button-tool-bar-fill-invisible': { color: 'overlay0', alpha: 0.36 },
    '--dsw-alias-button-tool-bar-hover': { color: 'overlay1', alpha: 0.6 },
    '--dsw-alias-label-primary-bluish': 'text',
    '--dsw-alias-label-primary-dimmed': 'subtext0',
    '--dsw-alias-label-primary-foreground': 'crust',
    '--dsw-alias-label-primary-inverted': 'surface0',
    '--shiki-foreground': 'text',
    '--shiki-background': 'mantle',
    '--shiki-token-constant': 'peach',
    '--shiki-token-string': 'green',
    // comment/punctuation brightened to match catppuccin-vscode (its
    // comment is overlay2, punctuation inherits text); link goes blue;
    // the diff trio was completely unmapped before
    '--shiki-token-comment': 'overlay2',
    '--shiki-token-keyword': 'mauve',
    '--shiki-token-parameter': 'maroon',
    '--shiki-token-function': 'blue',
    '--shiki-token-string-expression': 'green',
    '--shiki-token-punctuation': 'subtext0',
    '--shiki-token-link': 'blue',
    '--shiki-token-inserted': 'green',
    '--shiki-token-deleted': 'red',
    '--shiki-token-changed': 'peach',
  },
  light: {
    '--dsw-static-blue-450': 'mauve',
    '--dsw-static-blue-500': 'mauve',
    '--dsw-ctp-sky': 'sky',
    '--dsw-ctp-peach': 'peach',
    '--dsw-ctp-lavender': 'lavender',
    '--dsw-ctp-blue': 'blue',
    '--dsw-ctp-pink': 'pink',
    '--dsw-ctp-teal': 'teal',
    '--dsw-ctp-sapphire': 'sapphire',
    '--dsw-ctp-rosewater': 'rosewater',
    '--dsw-ctp-flamingo': 'flamingo',
    '--dsw-ctp-maroon': 'maroon',
    '--dsw-alias-bg-base': 'base',
    '--dsw-alias-bg-layer-1': 'base',
    '--dsw-alias-bg-layer-2': 'mantle',
    '--dsw-alias-bg-layer-3': 'surface0',
    '--dsw-alias-bg-overlay': 'base',
    '--dsw-alias-bg-mask-1': { color: 'crust', alpha: 0.24 },
    '--dsw-alias-bg-mask-2': { color: 'crust', alpha: 0.12 },
    '--dsw-alias-bg-mask-3': { color: 'crust', alpha: 0.48 },
    '--dsw-alias-bg-module-platform': 'mantle',
    '--dsw-alias-bg-multi-select': 'mantle',
    '--dsw-alias-bg-skeleton': { color: 'surface0', alpha: 0.04 },
    '--dsw-alias-border-l1': { color: 'overlay0', alpha: 0.3 },
    '--dsw-alias-border-l2': { color: 'overlay1', alpha: 0.5 },
    '--dsw-alias-border-l3': { color: 'overlay1', alpha: 0.6 },
    '--dsw-alias-border-l4': { color: 'overlay1', alpha: 0.75 },
    '--dsw-alias-label-primary': 'text',
    '--dsw-alias-label-secondary': 'subtext0',
    '--dsw-alias-label-tertiary': 'subtext1',
    '--dsw-alias-label-caption': 'subtext1',
    '--dsw-alias-label-dimmed': 'subtext1',
    '--dsw-alias-brand-primary': 'mauve',
    '--dsw-alias-brand-text': 'base',
    '--dsw-alias-button-primary-hover': 'lavender',
    '--dsw-alias-button-primary-dimmed': 'mantle',
    '--dsw-alias-button-elevated-fill': 'base',
    '--dsw-alias-button-floating-fill': 'base',
    '--dsw-alias-button-floating-hover': 'mantle',
    '--dsw-alias-button-ghost-active-border': 'surface1',
    '--dsw-alias-button-ghost-active-fill': 'mantle',
    '--dsw-alias-button-ghost-active-hover': 'surface0',
    '--dsw-alias-state-business-primary': 'mauve',
    '--dsw-alias-state-business-tertiary': 'mantle',
    '--dsw-alias-state-error-primary': 'red',
    '--dsw-alias-state-error-secondary': 'red',
    '--dsw-alias-state-success-primary': 'green',
    '--dsw-alias-state-success-secondary': 'green',
    '--dsw-alias-state-success-tertiary': 'mantle',
    '--dsw-alias-state-warn-label': 'peach',
    '--dsw-alias-state-warn-primary': 'peach',
    '--dsw-alias-state-warn-secondary': 'peach',
    '--dsw-alias-state-warn-tertiary': 'mantle',
    '--dsw-alias-interactive-bg-hover': { color: 'surface0', alpha: 0.3 },
    '--dsw-alias-interactive-bg-active': { color: 'surface1', alpha: 0.4 },
    '--dsw-alias-interactive-bg-hover-accent': { color: 'mauve', alpha: 0.1 },
    '--dsw-alias-interactive-bg-hover-danger': { color: 'red', alpha: 0.05 },
    '--dsw-alias-interactive-bg-hover-solid': 'mantle',
    '--dsw-alias-markdown-code-block': 'mantle',
    '--dsw-alias-markdown-code-block-banner': 'mantle',
    '--dsw-alias-markdown-code-segment-selected': 'base',
    '--dsw-alias-markdown-code-segment-unselected': 'mantle',
    '--dsw-alias-markdown-citation': 'mantle',
    '--dsw-alias-markdown-inline-code': 'base',
    '--dsw-alias-markdown-placeholder': 'mantle',
    '--dsw-alias-markdown-tag': 'mantle',
    '--dsw-alias-toast-bg': 'surface0',
    '--dsw-alias-tooltip-bg': 'surface1',
    '--dsw-specific-sidebar-fill': 'mantle',
    '--dsw-specific-sidebar-nav-item-active': 'surface1',
    '--dsw-specific-sidebar-nav-item-active-accent': { color: 'mauve', alpha: 0.2 },
    '--dsw-specific-sidebar-nav-item-hover': 'surface0',
    '--dsw-specific-bubble': 'mantle',
    '--dsw-specific-bubble-highlight': 'surface0',
    '--dsw-specific-input-major': 'base',
    '--dsw-specific-login-input': 'mantle',
    '--dsw-specific-menu': 'mantle',
    '--dsw-specific-selector': 'surface0',
    '--dsw-specific-tip': 'mantle',
    '--dsw-alias-separator-primary': { color: 'mauve', alpha: 0.7 },
    '--dsw-alias-scrollbar-bg-l1': 'surface0',
    '--dsw-alias-scrollbar-bg-l2': 'surface1',
    '--dsw-alias-scrollbar-hover-l1': 'surface2',
    '--dsw-alias-scrollbar-hover-l2': 'surface2',
    '--dsw-alias-bg-mask-photo': 'rgba(0, 0, 0, 0.88)',
    '--dsw-alias-bg-mask-drop': 'rgba(255, 255, 255, 0.7)',
    '--dsw-alias-border-inverted': 'rgba(0, 0, 0, 0)',
    '--dsw-alias-border-inverted2': 'rgba(0, 0, 0, 0)',
    '--dsw-alias-border-l2-darkmode-thin': { color: 'overlay1', alpha: 0.35 },
    '--dsw-alias-brand-primary-invert': 'text',
    '--dsw-alias-brand-primary-new-colorprimary-new-color': 'mauve',
    '--dsw-alias-button-contrast-fill': 'text',
    '--dsw-alias-button-info-fill': 'mauve',
    '--dsw-alias-button-info-hover': { mix: ['mauve', 'base'], t: 0.6 },
    '--dsw-alias-button-primary-fill': 'mauve',
    '--dsw-alias-button-tool-bar-fill': { color: 'overlay1', alpha: 0.5 },
    '--dsw-alias-button-tool-bar-fill-invisible': { color: 'overlay1', alpha: 0.36 },
    '--dsw-alias-button-tool-bar-hover': { color: 'overlay2', alpha: 0.6 },
    '--dsw-alias-label-primary-bluish': 'text',
    '--dsw-alias-label-primary-dimmed': 'subtext0',
    '--dsw-alias-label-primary-foreground': 'base',
    '--dsw-alias-label-primary-inverted': 'base',
    '--shiki-foreground': 'text',
    '--shiki-background': 'mantle',
    '--shiki-token-constant': 'peach',
    '--shiki-token-string': 'green',
    '--shiki-token-comment': 'overlay1',
    '--shiki-token-keyword': 'mauve',
    '--shiki-token-parameter': 'maroon',
    '--shiki-token-function': 'blue',
    '--shiki-token-string-expression': 'green',
    '--shiki-token-punctuation': 'subtext1',
    '--shiki-token-link': 'blue',
    '--shiki-token-inserted': 'green',
    '--shiki-token-deleted': 'red',
    '--shiki-token-changed': 'peach',
  },
}

const FLAVORS = [
  { key: 'latte', id: 'catppuccin-latte', colorScheme: 'light' },
  { key: 'frappe', id: 'catppuccin-frappe', colorScheme: 'dark' },
  { key: 'macchiato', id: 'catppuccin-macchiato', colorScheme: 'dark' },
  { key: 'mocha', id: 'catppuccin-mocha', colorScheme: 'dark' },
]

// Static color ladder plans: the official --dsw-static-* steps mapped onto
// the Catppuccin ramp. `neutral` covers neutral-bluish / neutral (00 =
// lightest, 1000 = darkest). Functional families map their 500 step to the
// Catppuccin accent and derive lighter/darker steps by mixing toward the
// scheme's light text or deep base (never out-of-palette colors). A plain
// string is a palette color name; [color, against, pct] is color-mix.
const STATIC_LADDERS = {
  dark: {
    // Interpolated so every step is unique (no flattened ladder); the ramp
    // runs lightest (00) to darkest (1000).
    neutral: {
      '00': 'text',
      '50': ['text', 'subtext1', 50],
      '60': 'subtext1',
      '75': ['subtext1', 'subtext0', 50],
      '100': 'subtext0',
      '150': ['subtext0', 'overlay2', 50],
      '200': 'overlay2',
      '250': ['overlay2', 'overlay1', 50],
      '300': 'overlay1',
      '400': ['overlay1', 'overlay0', 50],
      '500': 'surface2',
      '550': ['surface2', 'surface1', 50],
      '600': 'surface1',
      '700': 'surface0',
      '750': ['surface0', 'base', 50],
      '800': ['surface0', 'base', 25],
      '850': 'base',
      '875': ['base', 'mantle', 50],
      '900': 'mantle',
      '950': ['mantle', 'crust', 50],
      '1000': 'crust',
    },
    deepseek: {
      '50': ['mauve', 'text', 55], '100': ['mauve', 'text', 35], '200': 'lavender',
      '300': ['mauve', 'base', 70], '400': 'mauve', '450': 'mauve', '500': 'mauve',
      '600': ['mauve', 'base', 60], '700-delete': ['mauve', 'base', 45],
      '800': ['mauve', 'base', 30], '900': ['mauve', 'base', 20],
    },
    blue: {
      '50': ['blue', 'text', 55], '50p': ['blue', 'text', 45], '75': ['blue', 'text', 35],
      '100': ['blue', 'text', 25], '300': ['blue', 'base', 75], '400': ['blue', 'base', 85],
      '450': ['blue', 'base', 90], '500': 'blue', '600': ['blue', 'base', 70],
      '800': ['blue', 'base', 50], '900': ['blue', 'base', 35], '950': ['blue', 'base', 25],
    },
    green: { '100': ['green', 'text', 30], '400': ['green', 'base', 75], '500': 'green', '900': ['green', 'base', 35] },
    red: { '50': ['red', 'text', 40], '100': ['red', 'text', 25], '400': ['red', 'base', 75], '500': 'red', '600': ['red', 'base', 65], '900': ['red', 'base', 35] },
    amber: { '100': ['yellow', 'text', 30], '400': ['yellow', 'base', 85], '500': 'peach', '600': 'peach', '900': ['peach', 'base', 40] },
  },
  light: {
    neutral: {
      '00': 'base',
      '50': ['base', 'mantle', 50],
      '60': 'mantle',
      '75': ['mantle', 'surface0', 50],
      '100': 'surface0',
      '150': ['surface0', 'surface1', 50],
      '200': 'surface1',
      '250': ['surface1', 'surface2', 50],
      '300': 'surface2',
      '400': ['surface2', 'overlay0', 50],
      '500': 'overlay1',
      '550': ['overlay1', 'overlay2', 50],
      '600': 'overlay2',
      '700': ['overlay2', 'subtext0', 50],
      '750': 'subtext0',
      '800': ['subtext0', 'subtext1', 50],
      '850': 'subtext1',
      '875': ['subtext1', 'text', 50],
      '900': 'text',
      '950': 'text',
      '1000': 'text',
    },
    deepseek: {
      '50': ['mauve', 'base', 60], '100': ['mauve', 'base', 40], '200': 'lavender',
      '300': ['mauve', 'base', 75], '400': 'mauve', '450': 'mauve', '500': 'mauve',
      '600': ['mauve', 'text', 65], '700-delete': ['mauve', 'text', 45],
      '800': ['mauve', 'text', 30], '900': ['mauve', 'text', 18],
    },
    blue: {
      '50': ['blue', 'base', 60], '50p': ['blue', 'base', 48], '75': ['blue', 'base', 38],
      '100': ['blue', 'base', 28], '300': ['blue', 'base', 78], '400': ['blue', 'base', 88],
      '450': ['blue', 'base', 92], '500': 'blue', '600': ['blue', 'text', 70],
      '800': ['blue', 'text', 50], '900': ['blue', 'text', 35], '950': ['blue', 'text', 25],
    },
    green: { '100': ['green', 'base', 35], '400': ['green', 'base', 80], '500': 'green', '900': ['green', 'text', 35] },
    red: { '50': ['red', 'base', 45], '100': ['red', 'base', 28], '400': ['red', 'base', 78], '500': 'red', '600': ['red', 'text', 68], '900': ['red', 'text', 35] },
    amber: { '100': ['yellow', 'base', 35], '400': ['yellow', 'base', 88], '500': 'peach', '600': 'peach', '900': ['peach', 'text', 40] },
  },
}

const buildStaticTokens = (colors, scheme) => {
  const tokens = {}
  const ladders = STATIC_LADDERS[scheme]
  for (const [family, plan] of Object.entries(ladders)) {
    const names = family === 'neutral' ? ['neutral-bluish', 'neutral'] : [family]
    for (const [step, value] of Object.entries(plan)) {
      for (const name of names) {
        const key = `--dsw-static-${name}-${step}`
        if (typeof value === 'string') {
          tokens[key] = colors[value].hex
        } else {
          const [color, against, pct] = value
          tokens[key] = `color-mix(in srgb, ${colors[color].hex} ${pct}%, ${colors[against].hex})`
        }
      }
    }
  }
  return tokens
}

const buildTokens = (colors, scheme) => {
  const tokens = {}
  // Static ladder first; explicit TOKEN_MAPS entries (e.g. the mauve pins for
  // blue-450/500) win over generated ladder steps.
  Object.assign(tokens, buildStaticTokens(colors, scheme))
  for (const [token, mapping] of Object.entries(TOKEN_MAPS[scheme])) {
    if (typeof mapping === 'string') {
      tokens[token] = colors[mapping] ? colors[mapping].hex : mapping
    } else if ('mix' in mapping) {
      const [a, b] = mapping.mix
      tokens[token] = mixHex(colors[a].hex, colors[b].hex, mapping.t)
    } else {
      tokens[token] = rgba(colors[mapping.color].hex, mapping.alpha)
    }
  }
  return tokens
}

const skins = FLAVORS.map(({ key, id, colorScheme }) => {
  const colors = palette[key].colors
  const skin = {
    id,
    name: palette[key].name,
    colorScheme,
    tokens: buildTokens(colors, colorScheme),
  }
  writeFileSync(
    join(root, 'themes', `${id}.json`),
    JSON.stringify(skin, null, 2) + '\n',
  )
  return skin
})

const template = readFileSync(join(root, 'lib', 'client.tpl.js'), 'utf8')
const client = template.replace('__SKINS__', JSON.stringify(skins, null, 2))
writeFileSync(join(root, 'lib', 'client.js'), client)

console.log(`generated ${skins.length} themes and lib/client.js`)
