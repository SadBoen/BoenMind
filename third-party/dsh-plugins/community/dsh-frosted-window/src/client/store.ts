import { DEFAULT_KNOBS, type FrostedKnobs } from './knobs.ts'

/** Settings-section mirror of the live frosted surface. */
export interface FrostedState extends FrostedKnobs {
  hasImage: boolean
  previewUrl: string | null
  fileName: string | null
  width: number
  height: number
  dirty: boolean
  busy: boolean
  error: string | null
  revision: number
}

export const INITIAL_STATE: FrostedState = {
  ...DEFAULT_KNOBS,
  hasImage: false,
  previewUrl: null,
  fileName: null,
  width: 0,
  height: 0,
  dirty: false,
  busy: false,
  error: null,
  revision: -1,
}

/** Tiny store — avoids a hard runtime import in unit tests. */
export interface FrostedStore {
  get(): FrostedState
  set(next: FrostedState): void
  subscribe(listener: () => void): () => void
}

/** Create an in-memory store for the settings section. */
export function createFrostedStore(init: FrostedState = INITIAL_STATE): FrostedStore {
  let state = init
  const listeners = new Set<() => void>()
  return {
    get: () => state,
    set: (next) => {
      state = next
      for (const listener of listeners) listener()
    },
    subscribe: (listener) => {
      listeners.add(listener)
      return () => { listeners.delete(listener) }
    },
  }
}
