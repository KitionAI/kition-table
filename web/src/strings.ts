import type { Catalog } from '@kition/sdk/editor'

/** The shell's own strings (`en-US`; @kition/sdk/editor translates its own). The editor's are in `locales/`. */
const enUS: Catalog = {
  openFailed: 'The table could not be opened',
}

export const CATALOGS: Record<string, Catalog> = { 'en-US': enUS }
