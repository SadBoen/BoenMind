import { describe, expect, it, vi } from 'vitest'
import { PACKAGE_ID } from '../src/client/constants.ts'
import { apply, inject } from '../src/client/index.ts'

function mockCtx() {
  const effects: Array<() => void> = []
  const tokens: unknown[] = []
  const sections: unknown[] = []
  const listeners = new Map<string, Array<(...args: unknown[]) => void>>()
  const ctx = {
    theme: {
      getTheme: () => ({ active: { colorScheme: 'light' as const } }),
      overrideTokens: vi.fn((source: string, layer: unknown) => {
        tokens.push({ source, layer })
        return () => { /* retract */ }
      }),
    },
    locale: {
      register: vi.fn(() => () => { /* retract */ }),
      bind: () => (key: string) => key,
    },
    slots: {
      inject: vi.fn((_name: string, register: () => unknown) => {
        sections.push(register())
        return () => { /* retract */ }
      }),
      register: vi.fn((meta: unknown, component: unknown) => {
        return { meta, component }
      }),
    },
    effect: (factory: () => unknown, _label?: string) => {
      const dispose = factory()
      if (typeof dispose === 'function') effects.push(dispose as () => void)
    },
    on: (event: string, handler: (...args: unknown[]) => void) => {
      const list = listeners.get(event) ?? []
      list.push(handler)
      listeners.set(event, list)
      return () => {
        listeners.set(event, (listeners.get(event) ?? []).filter(item => item !== handler))
      }
    },
  }
  return { ctx, effects, tokens, sections, listeners }
}

describe('apply', () => {
  it('declares the services the fiber must wait for', () => {
    expect(inject).toEqual(['slots', 'locale', 'theme'])
  })

  it('registers locale, a settings section, and a theme/change listener', () => {
    const { ctx, sections } = mockCtx()
    apply(ctx as never)
    expect(ctx.locale.register).toHaveBeenCalled()
    expect(ctx.slots.inject).toHaveBeenCalledWith('settings.general.item', expect.any(Function))
    expect(ctx.slots.inject).toHaveBeenCalledWith('settings.section', expect.any(Function))
    expect(sections).toHaveLength(2)
    const ids = sections.map(entry => (entry as { meta: { id: string } }).meta.id)
    expect(ids).toContain(PACKAGE_ID)
    expect(ids).toContain('frosted-window')
  })
})
