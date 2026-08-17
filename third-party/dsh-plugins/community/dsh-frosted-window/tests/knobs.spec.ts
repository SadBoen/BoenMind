import { describe, expect, it, beforeEach } from 'vitest'
import { KNOBS_KEY } from '../src/client/constants.ts'
import {
  DEFAULT_KNOBS, clampKnob, loadKnobs, normalizeKnobs, saveKnobs,
} from '../src/client/knobs.ts'

describe('knobs', () => {
  beforeEach(() => { localStorage.clear() })

  it('clamps numeric knobs into published ranges', () => {
    expect(clampKnob('glassOpacity', 2)).toBe(0.82)
    expect(clampKnob('glassOpacity', -1)).toBe(0.18)
    expect(clampKnob('blurPx', 3)).toBe(8)
    expect(clampKnob('blurPx', 99)).toBe(64)
    expect(clampKnob('saturate', Number.NaN)).toBe(DEFAULT_KNOBS.saturate)
    expect(clampKnob('dim', 1)).toBe(0.65)
  })

  it('treats missing enabled as on and fills defaults', () => {
    expect(normalizeKnobs(null)).toEqual(DEFAULT_KNOBS)
    expect(normalizeKnobs({ enabled: false, glassOpacity: 9 }).enabled).toBe(false)
    expect(normalizeKnobs({ glassOpacity: 9 }).glassOpacity).toBe(0.82)
  })

  it('round-trips through localStorage', () => {
    saveKnobs({ ...DEFAULT_KNOBS, enabled: false, blurPx: 40 })
    expect(loadKnobs()).toMatchObject({ enabled: false, blurPx: 40 })
    expect(JSON.parse(localStorage.getItem(KNOBS_KEY)!).blurPx).toBe(40)
  })

  it('returns defaults when stored JSON is corrupt', () => {
    localStorage.setItem(KNOBS_KEY, '{')
    expect(loadKnobs()).toEqual(DEFAULT_KNOBS)
  })
})
