import { BODY_ATTR, PACKAGE_ID } from './constants.ts'
import { GLASS_CSS } from './glass-css.ts'
import type { FrostedKnobs } from './knobs.ts'

export type ColorScheme = 'light' | 'dark'

/** One applied wallpaper + glass state. */
export interface FrostedSurface {
  knobs: FrostedKnobs
  objectUrl: string | null
  scheme: ColorScheme
}

/**
 * Owns the wallpaper plate, dim veil, scoped stylesheet, and body attribute.
 * Retracts only what it wrote (ThemePresenter contract).
 */
export class FrostedPresenter {
  private styleEl: HTMLStyleElement | undefined
  private wallpaperEl: HTMLDivElement | undefined
  private dimEl: HTMLDivElement | undefined
  private objectUrl: string | undefined

  /** Project one surface onto the document. Passing a disabled/empty surface retracts. */
  apply(surface: FrostedSurface): void {
    const active = surface.knobs.enabled && surface.objectUrl !== null
    if (!active) {
      this.retractChrome()
      return
    }
    this.ensureChrome()
    const wallpaper = this.wallpaperEl
    const dim = this.dimEl
    if (wallpaper === undefined || dim === undefined) return
    wallpaper.style.backgroundImage = `url(${JSON.stringify(surface.objectUrl)})`
    document.body.setAttribute(BODY_ATTR, surface.scheme)
    document.body.style.setProperty('--fw-blur', `${surface.knobs.blurPx}px`)
    document.body.style.setProperty('--fw-saturate', `${Math.round(surface.knobs.saturate * 100)}%`)
    document.body.style.setProperty('--fw-dim', String(surface.knobs.dim))
  }

  /** Remember a blob URL so dispose can revoke it. Callers revoke the previous URL after React paints. */
  adoptObjectUrl(url: string | undefined): void {
    this.objectUrl = url
  }

  /** Current adopted object URL, if any. */
  currentObjectUrl(): string | undefined {
    return this.objectUrl
  }

  /** Retract every node, attribute, custom property, and object URL. */
  dispose(): void {
    this.retractChrome()
    if (this.objectUrl !== undefined) {
      URL.revokeObjectURL(this.objectUrl)
      this.objectUrl = undefined
    }
  }

  private ensureChrome(): void {
    if (this.styleEl === undefined || !this.styleEl.isConnected) {
      const style = document.createElement('style')
      style.dataset.plugin = PACKAGE_ID
      style.textContent = GLASS_CSS
      document.head.append(style)
      this.styleEl = style
    }
    if (this.wallpaperEl === undefined || !this.wallpaperEl.isConnected) {
      const plate = document.createElement('div')
      plate.setAttribute(`${BODY_ATTR}-wallpaper`, '')
      document.body.prepend(plate)
      this.wallpaperEl = plate
    }
    if (this.dimEl === undefined || !this.dimEl.isConnected) {
      const veil = document.createElement('div')
      veil.setAttribute(`${BODY_ATTR}-dim`, '')
      this.wallpaperEl.after(veil)
      this.dimEl = veil
    }
  }

  private retractChrome(): void {
    this.styleEl?.remove()
    this.styleEl = undefined
    this.wallpaperEl?.remove()
    this.wallpaperEl = undefined
    this.dimEl?.remove()
    this.dimEl = undefined
    document.body.removeAttribute(BODY_ATTR)
    document.body.style.removeProperty('--fw-blur')
    document.body.style.removeProperty('--fw-saturate')
    document.body.style.removeProperty('--fw-dim')
  }
}
