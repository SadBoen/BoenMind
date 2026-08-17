/**
 * Browser half: upload an image, persist it in IndexedDB, and project it as
 * a full-window frosted-glass theme through official ThemeRuntime overrides
 * plus a scoped stylesheet. Does not register a custom theme id — official
 * Light / Dark / System stay the preference authority.
 */
import type { Context } from '@deepseek-ai/cordis'
import type {} from '@deepseek-ai/dsh-client-locale/client'
import type {} from '@deepseek-ai/dsh-client-ui-theme/client'
import { PACKAGE_ID, LOCALE_NS } from './constants.ts'
import { FrostedSection, type FrostedSectionInjected } from './FrostedSection.tsx'
import { ImageValidationError, prepareWallpaper, wallpaperBlob, type WallpaperRecord } from './image.ts'
import { openImageStore, type ImageStore } from './image-store.ts'
import { loadKnobs, saveKnobs, normalizeKnobs, type FrostedKnobs } from './knobs.ts'
import { en, zh, type FrostedKey } from './locales.ts'
import { createFrostedStore, type FrostedState } from './store.ts'
import { glassTokenOverrides } from './tokens.ts'
import { FrostedPresenter, type ColorScheme } from './wallpaper.ts'

export const name = PACKAGE_ID
export const inject = ['slots', 'locale', 'theme']

interface ThemeFace {
  getTheme?: () => { active?: { colorScheme?: string } } | undefined
  overrideTokens?: (source: string, tokens: ReturnType<typeof glassTokenOverrides>) => unknown
}

interface LocaleFace {
  register(ns: string, dicts: { zh: typeof zh; en: typeof en }): () => void
  bind(ns: string): (key: FrostedKey) => string
}

interface SlotsFace {
  inject(name: string, register: () => unknown): () => void
  register(meta: Record<string, unknown>, component: unknown): () => void
}

interface ClientCtx extends Context {
  theme: ThemeFace
  locale: LocaleFace
  slots: SlotsFace
}

/** Client plugin body. */
export function apply(ctx: ClientCtx): void {
  const images: ImageStore = openImageStore()
  const presenter = new FrostedPresenter()
  const store = createFrostedStore({
    ...loadKnobs(),
    hasImage: false,
    previewUrl: null,
    fileName: null,
    width: 0,
    height: 0,
    dirty: false,
    busy: false,
    error: null,
    revision: 0,
  })
  let knobs = loadKnobs()
  let draft: WallpaperRecord | undefined
  let disposeTokens: (() => void) | undefined
  let mutation = 0
  let disposed = false
  let projecting = false

  const t = (key: FrostedKey): string => {
    try { return ctx.locale.bind(LOCALE_NS)(key) }
    catch { return zh[key] }
  }

  const publish = (patch: Partial<FrostedState>): void => {
    const current = store.get()
    store.set({ ...current, ...patch, revision: current.revision + 1 })
  }

  const schemeOf = (): ColorScheme => {
    try {
      return ctx.theme.getTheme?.()?.active?.colorScheme === 'dark' ? 'dark' : 'light'
    } catch {
      return 'light'
    }
  }

  const stackTokens = (live: boolean): void => {
    if (typeof disposeTokens === 'function') disposeTokens()
    disposeTokens = undefined
    if (!live || typeof ctx.theme.overrideTokens !== 'function') return
    const retract = ctx.theme.overrideTokens(PACKAGE_ID, glassTokenOverrides(knobs))
    disposeTokens = typeof retract === 'function' ? retract : undefined
  }

  const projectChrome = (): void => {
    if (disposed || projecting) return
    projecting = true
    try {
      const preview = store.get().previewUrl
      presenter.apply({ knobs, objectUrl: preview, scheme: schemeOf() })
    } finally {
      projecting = false
    }
  }

  const project = (restack: boolean): void => {
    if (disposed) return
    const live = knobs.enabled && store.get().previewUrl !== null
    projectChrome()
    if (restack) stackTokens(live)
  }

  const persistKnobs = (next: FrostedKnobs): void => {
    knobs = normalizeKnobs(next)
    publish({ ...knobs, dirty: true })
    project(true)
  }

  const adoptRecord = (record: WallpaperRecord | undefined, dirty: boolean): void => {
    if (disposed) return
    const previous = presenter.currentObjectUrl()
    draft = record
    if (record === undefined) {
      publish({
        hasImage: false, previewUrl: null, fileName: null, width: 0, height: 0, dirty, error: null,
      })
      presenter.adoptObjectUrl(undefined)
      if (previous !== undefined) requestAnimationFrame(() => { URL.revokeObjectURL(previous) })
      project(true)
      return
    }
    const url = URL.createObjectURL(wallpaperBlob(record))
    publish({
      hasImage: true,
      previewUrl: url,
      fileName: record.name,
      width: record.width,
      height: record.height,
      dirty,
      error: null,
    })
    presenter.adoptObjectUrl(url)
    if (previous !== undefined && previous !== url) {
      requestAnimationFrame(() => { URL.revokeObjectURL(previous) })
    }
    project(true)
  }

  const upload = async (file: File): Promise<void> => {
    const generation = ++mutation
    publish({ busy: true, error: null })
    try {
      const record = await prepareWallpaper(file)
      if (generation !== mutation || disposed) return
      adoptRecord(record, true)
    } catch (error) {
      if (generation !== mutation || disposed) return
      publish({ error: messageFor(error, t) })
    } finally {
      if (generation === mutation && !disposed) publish({ busy: false })
    }
  }

  const save = async (): Promise<void> => {
    const generation = ++mutation
    publish({ busy: true, error: null })
    try {
      saveKnobs(knobs)
      if (draft === undefined) await images.clear()
      else await images.put(draft)
      if (generation !== mutation || disposed) return
      publish({ dirty: false })
    } catch (error) {
      if (generation !== mutation || disposed) return
      publish({ error: messageFor(error, t) })
    } finally {
      if (generation === mutation && !disposed) publish({ busy: false })
    }
  }

  const remove = async (): Promise<void> => {
    const generation = ++mutation
    publish({ busy: true, error: null })
    try {
      await images.clear()
      saveKnobs(knobs)
      if (generation !== mutation || disposed) return
      adoptRecord(undefined, false)
    } catch (error) {
      if (generation !== mutation || disposed) return
      publish({ error: messageFor(error, t) })
    } finally {
      if (generation === mutation && !disposed) publish({ busy: false })
    }
  }

  ctx.effect(() => ctx.locale.register(LOCALE_NS, { zh, en }), `${PACKAGE_ID}: locale`)

  const injected = (): FrostedSectionInjected => ({
    store,
    t,
    setEnabled: enabled => { persistKnobs({ ...knobs, enabled }) },
    setKnob: (key, value) => { persistKnobs({ ...knobs, [key]: value }) },
    upload,
    save,
    remove,
  })

  ctx.effect(() => ctx.slots.inject('settings.general.item', () => ctx.slots.register({
    name: 'settings.general.item',
    id: 'frosted-window',
    order: 12,
    locale: LOCALE_NS,
    inject: injected,
  }, FrostedSection)), `${PACKAGE_ID}: general row`)

  ctx.effect(() => ctx.slots.inject('settings.section', () => ctx.slots.register({
    name: 'settings.section',
    id: PACKAGE_ID,
    order: 36,
    label: () => t('nav'),
    locale: LOCALE_NS,
    inject: injected,
  }, FrostedSection)), `${PACKAGE_ID}: settings`)

  ctx.effect(() => {
    const boot = mutation
    const off = ctx.on('theme/change', () => { project(false) })
    void images.get().then((record) => {
      if (disposed || mutation !== boot) return
      if (record !== undefined) adoptRecord(record, false)
      else project(true)
    }).catch((error: unknown) => {
      if (!disposed && mutation === boot) publish({ error: messageFor(error, t) })
    })
    return () => {
      disposed = true
      mutation += 1
      off()
      if (typeof disposeTokens === 'function') disposeTokens()
      disposeTokens = undefined
      presenter.dispose()
    }
  }, `${PACKAGE_ID}: surface`)
}

function messageFor(error: unknown, t: (key: FrostedKey) => string): string {
  if (error instanceof ImageValidationError && error.message.includes('unsupported')) return t('errorType')
  return t('errorGeneric')
}
