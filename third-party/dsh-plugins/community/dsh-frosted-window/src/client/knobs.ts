import { KNOBS_KEY } from './constants.ts'

/** User-tunable glass / wallpaper knobs. Image bytes live in IndexedDB. */
export interface FrostedKnobs {
  /** Apply wallpaper + glass when an image is stored. */
  enabled: boolean
  /** Glass fill opacity in [0.18, 0.82]. */
  glassOpacity: number
  /** Backdrop blur in px, [8, 64]. */
  blurPx: number
  /** Backdrop saturate multiplier, [1, 2]. */
  saturate: number
  /** Wallpaper dim veil in [0, 0.65]. */
  dim: number
}

export const DEFAULT_KNOBS: FrostedKnobs = {
  enabled: true,
  glassOpacity: 0.46,
  blurPx: 28,
  saturate: 1.55,
  dim: 0.28,
}

const RANGES: { [K in keyof Omit<FrostedKnobs, 'enabled'>]: readonly [number, number] } = {
  glassOpacity: [0.18, 0.82],
  blurPx: [8, 64],
  saturate: [1, 2],
  dim: [0, 0.65],
}

/**
 * Clamp one numeric knob into its published range.
 * @param key - numeric knob name.
 * @param value - raw number.
 */
export function clampKnob<K extends keyof typeof RANGES>(key: K, value: number): number {
  const [min, max] = RANGES[key]
  if (!Number.isFinite(value)) return DEFAULT_KNOBS[key]
  return Math.min(max, Math.max(min, value))
}

/**
 * Normalize a partial / unknown record into a complete knob set.
 * @param raw - persisted JSON or UI draft.
 */
export function normalizeKnobs(raw: unknown): FrostedKnobs {
  const input = raw !== null && typeof raw === 'object' ? raw as Record<string, unknown> : {}
  return {
    enabled: input.enabled !== false,
    glassOpacity: clampKnob('glassOpacity', Number(input.glassOpacity)),
    blurPx: clampKnob('blurPx', Number(input.blurPx)),
    saturate: clampKnob('saturate', Number(input.saturate)),
    dim: clampKnob('dim', Number(input.dim)),
  }
}

/** Read knobs from localStorage; missing or corrupt values become defaults. */
export function loadKnobs(): FrostedKnobs {
  try {
    const raw = localStorage.getItem(KNOBS_KEY)
    if (raw === null) return { ...DEFAULT_KNOBS }
    return normalizeKnobs(JSON.parse(raw) as unknown)
  } catch {
    return { ...DEFAULT_KNOBS }
  }
}

/** Persist a complete knob set. Failures stay local (private mode / quota). */
export function saveKnobs(knobs: FrostedKnobs): void {
  try {
    localStorage.setItem(KNOBS_KEY, JSON.stringify(normalizeKnobs(knobs)))
  } catch {
    // Persistence is best-effort; the live session still applies.
  }
}
