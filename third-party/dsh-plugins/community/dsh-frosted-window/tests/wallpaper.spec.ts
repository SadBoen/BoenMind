import { describe, expect, it, afterEach } from 'vitest'
import { BODY_ATTR } from '../src/client/constants.ts'
import { DEFAULT_KNOBS } from '../src/client/knobs.ts'
import { GLASS_CSS } from '../src/client/glass-css.ts'
import { FrostedPresenter } from '../src/client/wallpaper.ts'

describe('FrostedPresenter', () => {
  afterEach(() => {
    document.body.replaceChildren()
    document.head.querySelectorAll('style[data-plugin]').forEach(node => { node.remove() })
    document.body.removeAttribute(BODY_ATTR)
  })

  it('paints wallpaper, dim, stylesheet, and body attribute', () => {
    const presenter = new FrostedPresenter()
    presenter.apply({
      knobs: DEFAULT_KNOBS,
      objectUrl: 'blob:test',
      scheme: 'dark',
    })
    expect(document.body.getAttribute(BODY_ATTR)).toBe('dark')
    expect(document.body.style.getPropertyValue('--fw-blur')).toBe('28px')
    expect(document.querySelector(`[${BODY_ATTR}-wallpaper]`)?.getAttribute('style'))
      .toContain('blob:test')
    expect(document.querySelector(`[${BODY_ATTR}-dim]`)).not.toBeNull()
    expect(document.head.querySelector('style[data-plugin="dsh-frosted-window"]')?.textContent)
      .toContain('backdrop-filter')
    presenter.dispose()
    expect(document.body.hasAttribute(BODY_ATTR)).toBe(false)
    expect(document.querySelector(`[${BODY_ATTR}-wallpaper]`)).toBeNull()
    expect(document.head.querySelector('style[data-plugin="dsh-frosted-window"]')).toBeNull()
  })

  it('retracts when disabled or when the image is missing', () => {
    const presenter = new FrostedPresenter()
    presenter.apply({ knobs: DEFAULT_KNOBS, objectUrl: 'blob:test', scheme: 'light' })
    presenter.apply({ knobs: { ...DEFAULT_KNOBS, enabled: false }, objectUrl: 'blob:test', scheme: 'light' })
    expect(document.body.hasAttribute(BODY_ATTR)).toBe(false)
    presenter.apply({ knobs: DEFAULT_KNOBS, objectUrl: 'blob:test', scheme: 'light' })
    presenter.apply({ knobs: DEFAULT_KNOBS, objectUrl: null, scheme: 'light' })
    expect(document.querySelector(`[${BODY_ATTR}-wallpaper]`)).toBeNull()
    presenter.dispose()
  })

  it('frosts every column via ::before and never filters the settings dialog', () => {
    expect(GLASS_CSS).toMatch(/\*:has\(> \[data-slot='sidebar'\]\)::before/)
    expect(GLASS_CSS).toMatch(/\*:has\(> \[data-slot='conversation'\]\)::before/)
    expect(GLASS_CSS).toMatch(/\*:has\(> \[data-slot='details'\]\)::before/)
    expect(GLASS_CSS).toContain('border-right: none !important')
    expect(GLASS_CSS).toContain('#34c759')
    expect(GLASS_CSS).not.toMatch(/\[role='dialog'\]/)
    expect(GLASS_CSS).not.toMatch(/sidebar\.settings/)
    const sidebarSelf = /\[data-slot='sidebar'\]\s*\{[^}]*backdrop-filter/
    expect(GLASS_CSS).not.toMatch(sidebarSelf)
  })

  it('revokes the current object URL on dispose', () => {
    const revoked: string[] = []
    const original = URL.revokeObjectURL
    URL.revokeObjectURL = (url: string) => { revoked.push(url) }
    const presenter = new FrostedPresenter()
    presenter.adoptObjectUrl('blob:one')
    presenter.adoptObjectUrl('blob:two')
    presenter.dispose()
    URL.revokeObjectURL = original
    expect(revoked).toEqual(['blob:two'])
  })
})
