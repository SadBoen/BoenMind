import { IMAGE_DB, IMAGE_KEY, IMAGE_STORE } from './constants.ts'
import { sanitizeWallpaperRecord, type WallpaperRecord } from './image.ts'

/** Persistence face for the uploaded wallpaper blob. */
export interface ImageStore {
  get(): Promise<WallpaperRecord | undefined>
  put(record: WallpaperRecord): Promise<void>
  clear(): Promise<void>
}



/**
 * Open the IndexedDB-backed store. The database is created on first use.
 */
export function openImageStore(): ImageStore {
  return {
    get: async () => {
      const stored = await withStore('readonly', store =>
        requestToPromise<WallpaperRecord | undefined>(store.get(IMAGE_KEY)))
      return hydrate(stored)
    },
    put: record => withStore('readwrite', store => requestToPromise(store.put({
      bytes: record.bytes,
      mime: record.mime,
      name: record.name,
      width: record.width,
      height: record.height,
      updatedAt: record.updatedAt,
    } satisfies WallpaperRecord, IMAGE_KEY))),
    clear: () => withStore('readwrite', store => requestToPromise(store.delete(IMAGE_KEY))),
  }
}

function hydrate(stored: WallpaperRecord | undefined): WallpaperRecord | undefined {
  return sanitizeWallpaperRecord(stored)
}

async function withStore<T>(
  mode: IDBTransactionMode,
  use: (store: IDBObjectStore) => T | Promise<T>,
): Promise<T> {
  const db = await openDb()
  try {
    const tx = db.transaction(IMAGE_STORE, mode)
    const result = await use(tx.objectStore(IMAGE_STORE))
    await txDone(tx)
    return result
  } finally {
    db.close()
  }
}

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(IMAGE_DB, 1)
    req.onupgradeneeded = () => {
      if (!req.result.objectStoreNames.contains(IMAGE_STORE)) {
        req.result.createObjectStore(IMAGE_STORE)
      }
    }
    req.onsuccess = () => { resolve(req.result) }
    req.onerror = () => { reject(req.error ?? new Error('indexedDB open failed')) }
  })
}

function requestToPromise<T>(req: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => { resolve(req.result) }
    req.onerror = () => { reject(req.error ?? new Error('indexedDB request failed')) }
  })
}

function txDone(tx: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => { resolve() }
    tx.onerror = () => { reject(tx.error ?? new Error('indexedDB transaction failed')) }
    tx.onabort = () => { reject(tx.error ?? new Error('indexedDB transaction aborted')) }
  })
}
