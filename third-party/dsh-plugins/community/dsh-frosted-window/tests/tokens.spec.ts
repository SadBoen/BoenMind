import { describe, expect, it } from 'vitest'
import { DEFAULT_KNOBS } from '../src/client/knobs.ts'
import { glassTokenOverrides } from '../src/client/tokens.ts'

describe('glassTokenOverrides', () => {
  it('emits light/dark string pairs for every official fill it touches', () => {
    const tokens = glassTokenOverrides(DEFAULT_KNOBS)
    expect(Object.keys(tokens).length).toBeGreaterThan(8)
    for (const [name, modes] of Object.entries(tokens)) {
      expect(name.startsWith('--dsw-')).toBe(true)
      expect(typeof modes.light).toBe('string')
      expect(typeof modes.dark).toBe('string')
      expect(modes.light.startsWith('rgba(')).toBe(true)
      expect(modes.dark.startsWith('rgba(')).toBe(true)
    }
  })

  it('raises both palette alphas when glass density increases', () => {
    const thin = glassTokenOverrides({ ...DEFAULT_KNOBS, glassOpacity: 0.2 })
    const thick = glassTokenOverrides({ ...DEFAULT_KNOBS, glassOpacity: 0.8 })
    const readAlpha = (value: string): number => Number(value.slice(value.lastIndexOf(',') + 1, -1))
    expect(readAlpha(thick['--dsw-alias-bg-layer-1']!.light))
      .toBeGreaterThan(readAlpha(thin['--dsw-alias-bg-layer-1']!.light))
    expect(readAlpha(thick['--dsw-alias-bg-layer-1']!.dark))
      .toBeGreaterThan(readAlpha(thin['--dsw-alias-bg-layer-1']!.dark))
  })
})
