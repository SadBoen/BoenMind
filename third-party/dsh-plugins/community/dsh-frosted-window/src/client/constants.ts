/** Package id — also the theme override layer source and the loader entry id. */
export const PACKAGE_ID = 'dsh-frosted-window'

/** Body attribute that scopes every injected style. */
export const BODY_ATTR = 'data-dsh-frosted-window'

/** Settings locale namespace. */
export const LOCALE_NS = 'settings.frosted-window'

/** localStorage key for knobs (never the image bytes). */
export const KNOBS_KEY = 'dsh-frosted-window:knobs'

/** IndexedDB database that holds the uploaded wallpaper blob. */
export const IMAGE_DB = 'dsh-frosted-window'
export const IMAGE_STORE = 'files'
export const IMAGE_KEY = 'wallpaper'

/** Allowed image MIME types at the upload boundary. */
export const ALLOWED_TYPES = ['image/jpeg', 'image/png', 'image/webp', 'image/gif'] as const
