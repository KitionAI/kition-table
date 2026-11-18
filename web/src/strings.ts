import type { Catalog } from '@kition/kit-editor'

/** The shell's own strings (`en-US`; kit-editor translates its own). The editor's are in `locales/`. */
const enUS: Catalog = {
  openFailed: 'The table could not be opened',
}

export const CATALOGS: Record<string, Catalog> = { 'en-US': enUS }
